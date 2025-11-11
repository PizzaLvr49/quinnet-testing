use std::hash::Hash;

use bevy::prelude::*;
use bevy_replicon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Event)]
/// Just a message for testing the api
pub struct TestMessage(pub String);

#[derive(Serialize, Deserialize, Debug, Event)]
/// Client -> Server event telling server about the clients new position on that frame
pub struct ClientMovementIntent(pub Vec2);

#[derive(Serialize, Deserialize, Debug, Component)]
#[require(Replicated)]
/// Replicated client data
pub struct ClientData {
    pub network_id: u64,
    pub pos: Vec2,
}

impl Hash for ClientData {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.network_id.hash(state);
    }
}
