use bevy::{prelude::*, state::app::StatesPlugin};
use bevy_enhanced_input::EnhancedInputPlugin;
use bevy_panic_handler::PanicHandlerBuilder;
use bevy_quinnet::{
    client::{
        ClientConnectionConfiguration, ClientConnectionConfigurationDefaultables, QuinnetClient,
        certificate::CertificateVerificationMode,
        connection::{ClientAddrConfiguration, ConnectionEvent},
    },
    server::{
        EndpointAddrConfiguration, QuinnetServer, ServerEndpointConfiguration,
        ServerEndpointConfigurationDefaultables, certificate::CertificateRetrievalMode,
    },
};
use bevy_replicon::{prelude::*, shared::backend::connected_client::NetworkId};
use bevy_replicon_quinnet::{ChannelsConfigurationExt, RepliconQuinnetPlugins};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv6Addr};

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

#[derive(Event, Serialize, Deserialize)]
struct YourClientId(u64);

fn main() {
    let mode = Mode::try_parse().unwrap_or_default();

    let mut app = App::new();
    app.insert_resource(mode.clone());

    configure_plugins(&mut app, &mode);
    configure_systems(&mut app);
    configure_replication(&mut app);

    app.run();
}

fn configure_plugins(app: &mut App, mode: &Mode) {
    if matches!(mode, Mode::Server { .. }) {
        app.add_plugins(MinimalPlugins)
            .add_plugins(bevy::log::LogPlugin::default())
            .add_plugins(StatesPlugin);
    } else {
        app.add_plugins(DefaultPlugins)
            .add_plugins((EnhancedInputPlugin, PanicHandlerBuilder::default().build()));
    }

    app.add_plugins((RepliconPlugins, RepliconQuinnetPlugins));
}

fn configure_replication(app: &mut App) {
    app.add_server_event::<YourClientId>(Channel::Ordered);
}

fn configure_systems(app: &mut App) {
    app.add_systems(Startup, setup);
    app.add_systems(Update, new_connection);
    app.add_systems(Last, disconnect_observer);

    app.add_observer(recieve_id);
}

fn new_connection(
    query: Query<(Entity, &NetworkId), Added<AuthorizedClient>>,
    mut commands: Commands,
) {
    for (entity, client) in query.iter() {
        info!("New Client: {:?}", client);
        commands.server_trigger(ToClients {
            mode: SendMode::Direct(ClientId::Client(entity)),
            message: YourClientId(client.get()),
        });
    }
}

fn recieve_id(id: On<YourClientId>) {
    info!("Client Id is: {}", id.0);
}

fn setup(
    mode: Res<Mode>,
    channels: Res<RepliconChannels>,
    mut server: ResMut<QuinnetServer>,
    mut client: ResMut<QuinnetClient>,
    commands: Commands,
) {
    info!("Starting {:?}", *mode);

    match &*mode {
        Mode::Server { port } => setup_server(*port, &channels, &mut server),
        Mode::Client { ip, port } => setup_client(*ip, *port, &channels, &mut client, commands),
        Mode::Local => {
            info!("Local mode: skipping networking");
        }
    }
}

fn setup_server(port: u16, channels: &RepliconChannels, server: &mut QuinnetServer) {
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

    info!("Server started on port {}", port);
}

fn setup_client(
    ip: IpAddr,
    port: u16,
    channels: &RepliconChannels,
    client: &mut QuinnetClient,
    mut commands: Commands,
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

    commands.spawn(Camera2d);
}

fn disconnect_observer(
    mut exit_events: MessageReader<AppExit>,
    mut client: ResMut<QuinnetClient>,
    mut server: ResMut<QuinnetServer>,
    mode: Res<Mode>,
) {
    for _event in exit_events.read() {
        match *mode {
            Mode::Client { .. } => disconnect_client(&mut client),
            Mode::Server { .. } => shutdown_server(&mut server),
            Mode::Local => info!("[Local] Shutting down - no networking to clean up"),
        }
    }
}

fn disconnect_client(client: &mut QuinnetClient) {
    info!("[Client] Disconnecting all connections...");
    let connection_ids: Vec<u64> = client.connections().map(|(id, _)| *id).collect();

    for connection_id in connection_ids {
        if let Err(e) = client.close_connection(connection_id) {
            warn!("Failed to close connection {}: {:?}", connection_id, e);
        }
    }
}

fn shutdown_server(server: &mut QuinnetServer) {
    info!("[Server] Shutting down endpoint...");
    if let Err(e) = server.stop_endpoint() {
        warn!("Failed to stop endpoint: {:?}", e);
    }
}
