use crate::ecs;
use crate::prelude::*;
use crate::server::assets;
use crate::server::entity;

/// Client component types
pub enum ClientComponent {
    /// Server side components which can
    /// also be applied to the client.
    Server(entity::ServerComponent),
    /// Modifies the animation speed based on the current movement speed
    AnimateMovement {
        /// The animation speed modifier
        modifier: f64,
    },
}

impl entity::ComponentCreator for ClientComponent {
    type Creator = super::ClientEntityCreator;
    type Raw = ServerClientComponentInfo;

    fn from_raw(log: &Logger, module: assets::ModuleKey<'_>, val: Self::Raw) -> Self {
        use self::ClientComponentInfo::*;
        match val {
            ServerClientComponentInfo::Server(server) => {
                ClientComponent::Server(entity::ServerComponent::from_raw(log, module, server))
            }
            ServerClientComponentInfo::Client(AnimateMovement { modifier }) => {
                ClientComponent::AnimateMovement { modifier }
            }
        }
    }

    /// Applies the components described by this creator.
    fn apply(&self, em: &mut ecs::Container, e: ecs::Entity) {
        match *self {
            // Not needed for the client
            ClientComponent::Server(ServerComponent::Vars { .. }) => {}
            ClientComponent::Server(ref sc) => sc.apply(em, e),
            ClientComponent::AnimateMovement { modifier } => {
                em.add_component::<super::AnimationMovementSpeed>(
                    e,
                    super::AnimationMovementSpeed { modifier },
                );
            }
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(untagged, rename_all = "snake_case")]
#[doc(hidden)]
pub enum ServerClientComponentInfo {
    Client(ClientComponentInfo),
    Server(entity::ServerComponentInfo),
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
#[doc(hidden)]
pub enum ClientComponentInfo {
    AnimateMovement { modifier: f64 },
}
