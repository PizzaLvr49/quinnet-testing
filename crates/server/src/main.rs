use bevy::app::ScheduleRunnerPlugin;
use bevy::log::LogPlugin;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use bevy_quinnet::server::{
    EndpointAddrConfiguration, QuinnetServer, ServerEndpointConfiguration,
    ServerEndpointConfigurationDefaultables, certificate::CertificateRetrievalMode,
};
use bevy_replicon::prelude::*;
use bevy_replicon::shared::backend::connected_client::NetworkId;
use bevy_replicon_quinnet::{ChannelsConfigurationExt, RepliconQuinnetPlugins};
use clap::Parser;
use shared::{ClientMovementIntent, Player};
use std::net::{IpAddr, Ipv6Addr};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Resource, Parser)]
struct Args {
    #[arg(short, long, default_value_t = Ipv6Addr::LOCALHOST.into())]
    ip: IpAddr,
    #[arg(short, long, default_value_t = 5000)]
    port: u16,
}

#[derive(Component, Default)]
struct MovementInput(Vec2);

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
    app.add_plugins(
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(
            1.0 / 64.0,
        ))),
    )
    .add_plugins((LogPlugin::default(), StatesPlugin))
    .add_plugins((RepliconPlugins, RepliconQuinnetPlugins));
}

fn configure_replication(app: &mut App) {
    app.add_client_event::<ClientMovementIntent>(Channel::Unreliable)
        .replicate::<Transform>()
        .replicate::<Player>();
}

fn configure_systems(app: &mut App) {
    app.add_systems(Startup, setup_server);
    app.add_systems(Update, (read_connected, check_shutdown, apply_movement));
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

fn read_connected(
    mut query: Query<(Entity, &NetworkId), Added<AuthorizedClient>>,
    mut commands: Commands,
) {
    for (entity, network_id) in query.iter_mut() {
        info!("Client connected: {}", network_id.get());

        commands.entity(entity).insert((
            Player {
                network_id: network_id.get(),
            },
            Transform::default(),
            MovementInput::default(),
        ));
    }
}

fn on_client_position(
    message: On<FromClient<ClientMovementIntent>>,
    mut query: Query<&mut MovementInput>,
) {
    if let Some(entity) = message.client_id.entity() {
        if let Ok(mut input) = query.get_mut(entity) {
            input.0 = message.0;
        }
    }
}

fn apply_movement(mut query: Query<(&MovementInput, &mut Transform)>, time: Res<Time>) {
    for (input, mut transform) in query.iter_mut() {
        transform.translation += Vec3::from((input.0, 0.0)) * time.delta_secs() * 100.0;
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
