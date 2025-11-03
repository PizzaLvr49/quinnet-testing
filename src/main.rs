use bevy::{prelude::*, state::app::StatesPlugin, time::common_conditions::on_timer};
use bevy_panic_handler::PanicHandlerBuilder;
use bevy_quinnet::{
    client::{
        ClientConnectionConfiguration, ClientConnectionConfigurationDefaultables, QuinnetClient,
        certificate::CertificateVerificationMode, connection::ClientAddrConfiguration,
    },
    server::{
        EndpointAddrConfiguration, QuinnetServer, ServerEndpointConfiguration,
        ServerEndpointConfigurationDefaultables, certificate::CertificateRetrievalMode,
    },
};
use bevy_replicon::prelude::*;
use bevy_replicon_quinnet::{ChannelsConfigurationExt, RepliconQuinnetPlugins};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, Ipv6Addr},
    time::Duration,
};

#[derive(Parser, Resource, Debug, Clone, PartialEq, Eq, Default)]
enum Mode {
    #[default]
    Local,
    Server {
        #[arg(short, long, default_value_t = 5000)]
        port: u16,
    },
    Client {
        #[arg(short, long, default_value_t = Ipv6Addr::LOCALHOST.into())]
        ip: IpAddr,
        #[arg(short, long, default_value_t = 5000)]
        port: u16,
    },
}

#[derive(Event, Serialize, Deserialize, Debug, Clone)]
struct ChatMessage {
    text: String,
}

#[derive(Component, Serialize, Deserialize, Debug, Clone)]
#[require(Replicated)]
struct MyComponent {
    num: u32,
}

fn main() {
    let mode = Mode::try_parse().unwrap_or_default();

    let mut app = App::new();
    app.insert_resource(mode.clone());

    if matches!(mode, Mode::Server { .. }) {
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::log::LogPlugin::default())
            .add_plugins(StatesPlugin);
    } else {
        app.add_plugins(DefaultPlugins);
    }

    app.add_plugins((
        RepliconPlugins,
        RepliconQuinnetPlugins,
        PanicHandlerBuilder::default().build(),
    ))
    .replicate::<MyComponent>()
    .add_client_event::<ChatMessage>(Channel::Ordered)
    .add_systems(Startup, setup)
    .add_systems(
        Update,
        (send_message_system, log_state).run_if(is_client_fn),
    )
    .add_systems(
        Update,
        update_state.run_if(on_timer(Duration::from_secs(2))),
    )
    .add_observer(receive_message_observer)
    .add_systems(Last, disconnect_observer)
    .run();
}

fn is_client_fn(mode: Res<Mode>) -> bool {
    matches!(*mode, Mode::Client { .. } | Mode::Local)
}

fn setup(
    commands: Commands,
    mode: Res<Mode>,
    channels: Res<RepliconChannels>,
    mut server: ResMut<QuinnetServer>,
    mut client: ResMut<QuinnetClient>,
) {
    info!("Starting {:?}", *mode);

    match &*mode {
        Mode::Server { port } => start_server(*port, &channels, &mut server, commands),
        Mode::Client { ip, port } => start_client(*ip, *port, &channels, &mut client),
        Mode::Local => {
            info!("Local mode: skipping networking");
            spawn_synced(commands);
        }
    }
}

fn start_server(
    port: u16,
    channels: &Res<RepliconChannels>,
    server: &mut ResMut<QuinnetServer>,
    commands: Commands,
) {
    server
        .start_endpoint(ServerEndpointConfiguration {
            addr_config: EndpointAddrConfiguration::from_ip(IpAddr::V6(Ipv6Addr::LOCALHOST), port),
            cert_mode: CertificateRetrievalMode::GenerateSelfSigned {
                server_hostname: Ipv6Addr::LOCALHOST.to_string(),
            },
            defaultables: ServerEndpointConfigurationDefaultables {
                send_channels_cfg: channels.server_configs(),
            },
        })
        .unwrap();

    spawn_synced(commands);
}

fn start_client(
    ip: IpAddr,
    port: u16,
    channels: &Res<RepliconChannels>,
    client: &mut ResMut<QuinnetClient>,
) {
    client
        .open_connection(ClientConnectionConfiguration {
            addr_config: ClientAddrConfiguration::from_ips(ip, port, Ipv6Addr::UNSPECIFIED, 0),
            cert_mode: CertificateVerificationMode::SkipVerification,
            defaultables: ClientConnectionConfigurationDefaultables {
                send_channels_cfg: channels.client_configs(),
            },
        })
        .unwrap();

    info!("Client connecting to [{ip}]:{port}");
}

fn spawn_synced(mut commands: Commands) {
    commands.spawn(MyComponent { num: 22 });
}

fn update_state(mode: Res<Mode>, mut query: Query<&mut MyComponent>) {
    if is_client_fn(mode) {
        return;
    }

    for mut component in query.iter_mut() {
        component.num += 1;
        info!("{:?}", component);
    }
}

fn log_state(mode: Res<Mode>, query: Query<&MyComponent>) {
    if !is_client_fn(mode) {
        return;
    }

    for component in query.iter() {
        info!("{:?}", component);
    }
}

fn send_message_system(
    mode: Res<Mode>,
    mut commands: Commands,
    time: Res<Time>,
    mut timer: Local<Option<Timer>>,
) {
    if !matches!(*mode, Mode::Client { .. } | Mode::Local) {
        return;
    }

    let timer = timer.get_or_insert_with(|| Timer::from_seconds(2.0, TimerMode::Repeating));
    if timer.tick(time.delta()).just_finished() {
        let message = ChatMessage {
            text: format!("Hello from client/local at {:.2}", time.elapsed_secs()),
        };

        if !matches!(*mode, Mode::Local) {
            commands.client_trigger(message.clone());
        }

        info!("Message: {:?}", message);
    }
}

fn receive_message_observer(trigger: On<FromClient<ChatMessage>>) {
    info!(
        "Server received from {:?}: {:?}",
        trigger.client_id, trigger.message
    );
}

fn disconnect_observer(
    mut exit_events: MessageReader<AppExit>,
    mut client: ResMut<QuinnetClient>,
    mut server: ResMut<QuinnetServer>,
    mode: Res<Mode>,
) {
    for _event in exit_events.read() {
        match *mode {
            Mode::Client { .. } => {
                info!("Disconnecting all client connections...");
                let connection_ids: Vec<u64> = client.connections().map(|(id, _)| *id).collect();
                for connection_id in connection_ids {
                    if let Err(e) = client.close_connection(connection_id) {
                        warn!("Failed to close connection {}: {:?}", connection_id, e);
                    }
                }
            }
            Mode::Server { .. } => {
                info!("Shutting down server endpoint...");
                if let Err(e) = server.stop_endpoint() {
                    warn!("Failed to stop endpoint: {:?}", e);
                }
            }
            Mode::Local => {
                info!("Local mode: no networking to disconnect");
            }
        }
    }
}
