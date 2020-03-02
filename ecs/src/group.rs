use super::*;

/// A group that can be iterated over in parallel
pub struct Group<'a, F: FetchableComponent<'a> + 'a> {
    pub(crate) entities: &'a RwLock<internal::EntityAllocator>,
    pub(crate) components: F,
    pub(crate) est_size: usize,
    pub(crate) mask: EntityMask,
    pub(crate) offset: usize,
}

impl<'a, F> Iterator for Group<'a, F>
where
    F: FetchableComponent<'a>,
{
    type Item = (Entity, F::Component);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        let entities = self.entities.read().unwrap();
        while self.offset < self.est_size {
            if self.mask.mask.get(self.offset) {
                let id = self.offset as u32;
                self.offset += 1;
                let entity = Entity {
                    id,
                    generation: entities.generations[id as usize],
                };
                let components = unsafe { self.components.fetch_component(id) };
                return Some((entity, components));
            } else {
                self.offset += 1;
            }
        }
        None
    }
}
