use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy_quinnet::server::{
    ConnectionEvent, EndpointAddrConfiguration, QuinnetServer, ServerEndpointConfiguration,
    ServerEndpointConfigurationDefaultables, certificate::CertificateRetrievalMode,
};
use bevy_replicon::prelude::*;
use bevy_replicon_quinnet::{ChannelsConfigurationExt, RepliconQuinnetPlugins};
use clap::Parser;
use shared::{ClientData, ClientMovementIntent};
use std::net::{IpAddr, Ipv6Addr};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};

#[derive(Resource, Parser)]
struct Args {
    #[arg(short, long, default_value_t = Ipv6Addr::LOCALHOST.into())]
    ip: IpAddr,
    #[arg(short, long, default_value_t = 5000)]
    port: u16,
}

#[derive(Resource)]
struct ShutdownReceiver(Arc<Mutex<Receiver<()>>>);

fn main() {
    let args = Args::parse();

    let (tx, rx) = channel();
    ctrlc::set_handler(move || {
        tx.send(()).expect("Could not send signal on channel.");
    })
    .expect("Error setting Ctrl-C handler");

    let mut app = App::new();
    app.insert_resource(args);
    app.insert_resource(ShutdownReceiver(Arc::new(Mutex::new(rx))));

    configure_plugins(&mut app);
    configure_systems(&mut app);
    configure_replication(&mut app);

    app.run();
}

fn configure_plugins(app: &mut App) {
    app.add_plugins(MinimalPlugins)
        .add_plugins((LogPlugin::default(), StatesPlugin))
        .add_plugins((RepliconPlugins, RepliconQuinnetPlugins));
}

fn configure_replication(app: &mut App) {
    app.add_client_event::<ClientMovementIntent>(Channel::Unreliable)
        .replicate::<ClientData>();
}

fn configure_systems(app: &mut App) {
    app.add_systems(Startup, setup_server);
    app.add_systems(Update, (read_connected, check_shutdown));
    app.add_systems(Last, disconnect_observer);

    app.add_observer(on_client_position);
}

fn check_shutdown(receiver: Res<ShutdownReceiver>, mut exit: MessageWriter<AppExit>) {
    if let Ok(rx) = receiver.0.lock() {
        if rx.try_recv().is_ok() {
            exit.write(AppExit::Success);
        }
    }
}

fn read_connected(mut reader: MessageReader<ConnectionEvent>, mut commands: Commands) {
    for message in reader.read() {
        info!("Client connected: {}", message.id);
        commands.spawn((ClientData {
            network_id: message.id,
            pos: Vec2::ZERO,
        },));
    }
}

fn on_client_position(
    message: On<FromClient<ClientMovementIntent>>,
    mut query: Query<&mut ClientData>,
) {
    let Some(entity) = message.client_id.entity() else {
        return;
    };
    if let Ok(mut client) = query.get_mut(entity) {
        client.pos = message.0; // Add position verification later
    }
}

fn setup_server(
    args: Res<Args>,
    channels: Res<RepliconChannels>,
    mut server: ResMut<QuinnetServer>,
) {
    let (ip, port) = (args.ip, args.port);

    server
        .start_endpoint(ServerEndpointConfiguration {
            addr_config: EndpointAddrConfiguration::from_ip(ip, port),
            cert_mode: CertificateRetrievalMode::GenerateSelfSigned {
                server_hostname: Ipv6Addr::LOCALHOST.to_string(),
            },
            defaultables: ServerEndpointConfigurationDefaultables {
                send_channels_cfg: channels.server_configs(),
            },
        })
        .unwrap();

    info!("Server listening on [{ip}]:{port}");
}

fn disconnect_observer(mut exit_events: MessageReader<AppExit>, mut server: ResMut<QuinnetServer>) {
    for _event in exit_events.read() {
        info!("Shutting down server...");
        if let Err(e) = server.stop_endpoint() {
            warn!("Failed to stop server endpoint: {:?}", e);
        }
    }
}
