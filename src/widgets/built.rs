//! Rebuilding a container's children only when what they show has changed.

use bevy::prelude::*;

/// What a container's children were last built from.
///
/// Kept on the container rather than in the rebuilding system's `Local`, so a
/// container spawned afresh is built afresh rather than matching what was
/// noted of the one before it, and each container remembers its own.
#[derive(Component)]
pub struct BuiltFrom<K: Send + Sync + 'static>(K);

/// Whether `container` has to be rebuilt to show `now`. When it does, `now`
/// is noted on it as what it is about to be built from.
pub fn needs_rebuild<K: PartialEq + Send + Sync + 'static>(
    commands: &mut Commands,
    built: &Query<&BuiltFrom<K>>,
    container: Entity,
    now: K,
) -> bool {
    if built.get(container).is_ok_and(|built| built.0 == now) {
        return false;
    }
    commands.entity(container).insert(BuiltFrom(now));
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ask(world: &mut World, container: Entity, now: u32) -> bool {
        let mut state =
            bevy::ecs::system::SystemState::<(Commands, Query<&BuiltFrom<u32>>)>::new(world);
        let (mut commands, built) = state.get_mut(world).unwrap();
        let rebuild = needs_rebuild(&mut commands, &built, container, now);
        state.apply(world);
        rebuild
    }

    #[test]
    fn a_container_is_rebuilt_only_when_what_it_shows_changes() {
        let mut world = World::new();
        let list = world.spawn_empty().id();
        assert!(ask(&mut world, list, 1), "never built");
        assert!(!ask(&mut world, list, 1));
        assert!(ask(&mut world, list, 2));

        // One spawned in its place starts with nothing noted.
        let fresh = world.spawn_empty().id();
        assert!(ask(&mut world, fresh, 2));
    }
}
