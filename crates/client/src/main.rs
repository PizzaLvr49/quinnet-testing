#![cfg_attr(not(feature = "dev"), windows_subsystem = "windows")]

use bevy::prelude::*;
use bevy_enhanced_input::prelude::*;
use bevy_panic_handler::PanicHandlerBuilder;
use bevy_quinnet::client::{
    ClientConnectionConfiguration, ClientConnectionConfigurationDefaultables, QuinnetClient,
    certificate::CertificateVerificationMode,
    connection::{ClientAddrConfiguration, ConnectionEvent},
};
use bevy_replicon::prelude::*;
use bevy_replicon_quinnet::{ChannelsConfigurationExt, RepliconQuinnetPlugins};
use clap::Parser;
use shared::{ClientData, ClientMovementIntent};
use std::net::{IpAddr, Ipv6Addr};

#[derive(Resource, Parser)]
struct Args {
    #[arg(short, long, default_value_t = Ipv6Addr::LOCALHOST.into())]
    ip: IpAddr,
    #[arg(short, long, default_value_t = 5000)]
    port: u16,
}

#[derive(Component)]
#[require(Transform)]
struct LocalPlayer;

#[derive(InputAction)]
#[action_output(Vec2)]
struct PlayerMovement;

fn main() {
    let args = Args::parse();

    let mut app = App::new();
    app.insert_resource(args);

    configure_plugins(&mut app);
    configure_systems(&mut app);
    configure_replication(&mut app);

    app.run();
}

fn configure_plugins(app: &mut App) {
    app.add_plugins(DefaultPlugins)
        .add_plugins((EnhancedInputPlugin, PanicHandlerBuilder::default().build()))
        .add_plugins((RepliconPlugins, RepliconQuinnetPlugins))
        .add_input_context::<LocalPlayer>();
}

fn configure_replication(app: &mut App) {
    app.add_client_event::<ClientMovementIntent>(Channel::Unreliable)
        .replicate::<ClientData>();
}

fn configure_systems(app: &mut App) {
    app.add_systems(Startup, setup_client);
    app.add_systems(
        Update,
        (read_connected, handle_new_players, handle_networked_players),
    );
    app.add_systems(Last, disconnect_observer);

    app.add_observer(on_input);
}

fn read_connected(mut reader: MessageReader<ConnectionEvent>, mut commands: Commands) {
    for message in reader.read() {
        let client_id = message.client_id.unwrap();
        info!("Client Id is: {}", client_id);

        commands.spawn((
            LocalPlayer,
            actions!(
                LocalPlayer[(
                    Action::<PlayerMovement>::new(),
                    Scale::splat(100.0),
                    DeadZone::default(),
                    SmoothNudge::new(32.0),
                    Bindings::spawn((
                        Cardinal::wasd_keys(),
                        Cardinal::arrows(),
                        Axial::left_stick(),
                    ))
                )]
            ),
            ClientData {
                network_id: client_id,
                pos: Vec2::ZERO,
            },
            Signature::of::<ClientData>(),
            Sprite::from_color(Color::linear_rgb(0.0, 1.0, 0.0), Vec2::splat(50.0)),
        ));
    }
}

fn setup_client(
    args: Res<Args>,
    channels: Res<RepliconChannels>,
    mut client: ResMut<QuinnetClient>,
    mut commands: Commands,
) {
    let (ip, port) = (args.ip, args.port);

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

fn handle_new_players(
    mut query: Query<Entity, (Added<ClientData>, Without<LocalPlayer>)>,
    mut commands: Commands,
) {
    for entity in query.iter_mut() {
        commands.entity(entity).insert((
            Sprite::from_color(Color::linear_rgb(1.0, 0.0, 0.0), Vec2::splat(50.0)),
            Signature::of::<ClientData>(),
        ));
    }
}

fn handle_networked_players(mut query: Query<(&mut Transform, &ClientData), Without<LocalPlayer>>) {
    for (mut transform, client_data) in query.iter_mut() {
        transform.translation = (client_data.pos, 0.0).into();
    }
}

fn on_input(
    movement: On<Fire<PlayerMovement>>,
    mut player_transform: Single<&mut Transform, With<LocalPlayer>>,
    mut commands: Commands,
) {
    player_transform.translation += Vec3::from((movement.value, 0.0));
    commands.client_trigger(ClientMovementIntent(player_transform.translation.xy()));
}

fn disconnect_observer(mut exit_events: MessageReader<AppExit>, mut client: ResMut<QuinnetClient>) {
    for _event in exit_events.read() {
        info!("Disconnecting all connections...");
        let connection_ids: Vec<u64> = client.connections().map(|(id, _)| *id).collect();

        for connection_id in connection_ids {
            if let Err(e) = client.close_connection(connection_id) {
                warn!("Failed to close connection {}: {:?}", connection_id, e);
            }
        }
    }
}
