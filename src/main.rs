use bevy::{prelude::*, state::app::StatesPlugin};
use bevy_enhanced_input::{
    EnhancedInputPlugin,
    action::Action,
    actions,
    prelude::{Axial, Bindings, Cardinal, DeadZone, Fire, InputAction, SmoothNudge},
};
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

#[derive(Debug, Component, Serialize, Deserialize, Clone)]
#[require(Replicated, Signature::of::<Player>())]
struct Player {
    id: u64,
    position: Vec2,
}

impl std::hash::Hash for Player {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

#[derive(Debug, Component)]
struct LocalPlayer;

#[derive(Event, Serialize, Deserialize, Clone, Copy)]
struct PositionUpdate {
    position: Vec2,
}

#[derive(InputAction)]
#[action_output(Vec2)]
struct PlayerMovement;

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
    app.replicate::<Player>();
    app.add_client_event::<PositionUpdate>(Channel::Ordered);
}

fn configure_systems(app: &mut App) {
    // Startup / Last as before
    app.add_systems(Startup, setup);
    app.add_systems(Last, disconnect_observer);

    app.add_systems(Update, handle_client_connections);
    app.add_systems(Update, handle_client_disconnections);
    app.add_systems(Update, mark_local_player);
    app.add_systems(Update, client_update_visual_positions);

    app.add_observer(new_client);
    app.add_observer(server_handle_position_updates);
    app.add_observer(client_handle_movement);
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

fn new_client(
    trigger: On<Add, AuthorizedClient>,
    mut commands: Commands,
    query: Query<&NetworkId>,
) {
    if let Ok(network_id) = query.get(trigger.entity) {
        let player_id = network_id.get();
        info!("New client authorized with NetworkId: {:?}", player_id);

        commands.spawn(Player {
            id: player_id,
            position: Vec2::ZERO,
        });
    } else {
        warn!("Client authorized but no NetworkId found!");
    }
}

fn handle_client_connections(query: Query<(Entity, &NetworkId), Added<ConnectedClient>>) {
    for (entity, network_id) in &query {
        info!(
            "Client connected - Entity: {:?}, NetworkId: {:?}",
            entity,
            network_id.get()
        );
    }
}

fn handle_client_disconnections(
    mut removed: RemovedComponents<ConnectedClient>,
    network_ids: Query<&NetworkId>,
) {
    for entity in removed.read() {
        if let Ok(network_id) = network_ids.get(entity) {
            info!(
                "Client disconnected - Entity: {:?}, NetworkId: {:?}",
                entity,
                network_id.get()
            );
        } else {
            info!("Client disconnected - Entity: {:?}", entity);
        }
    }
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

fn mark_local_player(
    mut commands: Commands,
    client: Res<QuinnetClient>,
    new_players: Query<(Entity, &Player), (Added<Player>, Without<LocalPlayer>)>,
) {
    if let Some((_connection_id, connection)) = client.connections().next() {
        if let Some(client_id) = connection.client_id() {
            for (entity, player) in &new_players {
                if player.id == client_id {
                    info!(
                        "Marking {:?} as LocalPlayer (client_id: {})",
                        entity, client_id
                    );
                    commands.entity(entity).insert((
                        LocalPlayer,
                        actions!(
                            LocalPlayer[(
                                Action::<PlayerMovement>::new(),
                                DeadZone::default(),
                                SmoothNudge::new(32.0),
                                Bindings::spawn((
                                    Cardinal::wasd_keys(),
                                    Axial::left_stick(),
                                    Cardinal::arrows(),
                                )),
                            )]
                        ),
                    ));
                }
            }
        }
    }
}

// Client: Handle movement locally and send position to server
fn client_handle_movement(
    movement: On<Fire<PlayerMovement>>,
    mut local_player: Query<&mut Player, With<LocalPlayer>>,
    mut commands: Commands,
    time: Res<Time>,
) {
    const MOVE_SPEED: f32 = 300.0;

    let Ok(mut player) = local_player.single_mut() else {
        return;
    };

    let movement_delta = movement.value.normalize_or_zero() * MOVE_SPEED * time.delta_secs();
    player.position += movement_delta;

    commands.client_trigger(PositionUpdate {
        position: player.position,
    });
}

// Server: Receive position updates from clients and update their Player
fn server_handle_position_updates(
    trigger: On<FromClient<PositionUpdate>>,
    mut players: Query<&mut Player>,
    clients: Query<Entity, With<ConnectedClient>>,
) {
    let ClientId::Client(client_entity) = trigger.client_id else {
        return;
    };

    if let Ok(parent) = clients.get(client_entity) {
        if let Ok(mut player) = players.get_mut(parent) {
            player.position = trigger.position;
            info!(
                "Updated position for player {} to {:?}",
                player.id, player.position
            );
        }
    }
}

// Client: Update visual positions based on replicated Player components
fn client_update_visual_positions(
    mut commands: Commands,
    players: Query<&Player, Changed<Player>>,
    mut visuals: Query<(Entity, &mut Transform, &VisualPlayer)>,
) {
    for player in &players {
        let mut found = false;
        for (_entity, mut transform, visual) in &mut visuals {
            if visual.player_id == player.id {
                transform.translation = player.position.extend(0.0);
                found = true;
                break;
            }
        }

        if !found {
            info!("Creating visual for player {}", player.id);
            commands.spawn((
                VisualPlayer {
                    player_id: player.id,
                },
                Transform::from_translation(player.position.extend(0.0)),
                Sprite::from_color(Color::WHITE, Vec2::splat(100.0)),
            ));
        }
    }
}

#[derive(Component)]
struct VisualPlayer {
    player_id: u64,
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
