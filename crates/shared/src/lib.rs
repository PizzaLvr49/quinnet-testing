use bevy::prelude::*;
use bevy_replicon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Event)]
/// Client -> Server event telling server about the client's new position
pub struct ClientMovementIntent(pub Vec2);

#[derive(Component)]
/// Marker component for the locally controlled player
pub struct LocalPlayer;

#[derive(Component, Serialize, Deserialize)]
#[require(Replicated)]
/// Marker component to identify player entities
pub struct Player {
    pub network_id: u64,
}
