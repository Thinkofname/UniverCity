
use super::*;

/// A group that can be iterated over in parallel
pub struct GroupPar<'a, F: FetchableComponent<'a> + 'a> {
    pub(crate) entities: &'a RwLock<internal::EntityAllocator>,
    pub(crate) components: F,
    pub(crate) est_size: usize,
    pub(crate) mask: EntityMask,
}

impl <'a, F> GroupPar<'a, F>
    where F: FetchableComponent<'a>,
          F::Component: Send + Sync
{
    /// Returns a parallel iterator over the entities in this group
    #[inline]
    pub fn par_iter(&'a self) -> GroupParIter<'a, F> {
        GroupParIter {
            entities: self.entities,
            components: &self.components,
            est_size: self.est_size,
            mask: &self.mask,
        }
    }
}

/// Parallel iterator over components
pub struct GroupParIter<'a, F: FetchableComponent<'a>> {
    pub(crate) entities: &'a RwLock<internal::EntityAllocator>,
    pub(crate) components: &'a F,
    pub(crate) est_size: usize,
    pub(crate) mask: &'a EntityMask,
}

unsafe impl <'a, F: FetchableComponent<'a> + 'a> Send for GroupParIter<'a, F> {}
unsafe impl <'a, F: FetchableComponent<'a> + 'a> Sync for GroupParIter<'a, F> {}

impl <'a, F> ParallelIterator for GroupParIter<'a, F>
    where F: FetchableComponent<'a>,
          F::Component: Send + Sync
{
    type Item = (Entity, F::Component);

    #[inline]
    fn drive_unindexed<C>(self, consumer: C) -> C::Result
        where C: UnindexedConsumer<Self::Item>
    {
        let producer = GroupProducer {
            entities: self.entities,
            components: self.components,
            mask: self.mask,
            start: 0,
            end: self.est_size,
        };
        bridge_unindexed(producer, consumer)
    }
}

struct GroupProducer<'a, F: FetchableComponent<'a>> {
    entities: &'a RwLock<internal::EntityAllocator>,
    components: &'a F,
    mask: &'a EntityMask,
    start: usize,
    end: usize,
}

unsafe impl <'a, F: FetchableComponent<'a> + 'a> Send for GroupProducer<'a, F> {}
unsafe impl <'a, F: FetchableComponent<'a> + 'a> Sync for GroupProducer<'a, F> {}

impl <'a, F> UnindexedProducer for GroupProducer<'a, F>
    where F: FetchableComponent<'a>,
          F::Component: Send + Sync
{
    type Item = (Entity, F::Component);

    #[inline]
    fn split(self) -> (Self, Option<Self>) {
        let diff = self.end - self.start;
        if diff <= 2 {
            (self, None)
        } else {
            let mid = diff / 2;
            (
                GroupProducer {
                    entities: self.entities,
                    components: self.components,
                    mask: self.mask,
                    start: self.start,
                    end: self.start + mid,
                },
                Some(GroupProducer {
                    entities: self.entities,
                    components: self.components,
                    mask: self.mask,
                    start: self.start + mid,
                    end: self.end,
                })
            )
        }
    }

    fn fold_with<Fo>(mut self, mut folder: Fo) -> Fo
        where Fo: Folder<Self::Item>
    {
        let entities = self.entities.read().unwrap();
        while self.start != self.end && !folder.full() {
            if self.mask.mask.get(self.start) {
                let id = self.start as u32;
                let entity = Entity {
                    id,
                    generation: entities.generations[id as usize],
                };
                let components = unsafe {
                    self.components.fetch_component(id)
                };
                folder = folder.consume((entity, components));
            }
            self.start += 1;
        }
        folder
    }
}

/// Parallel iterator over components
pub struct ReadParIter<'a, T: Component> {
    pub(crate) inner: &'a Read<'a, T>,
    pub(crate) est_size: usize,
    pub(crate) mask: &'a EntityMask,
}

impl <'a, T> ParallelIterator for ReadParIter<'a, T>
    where T: Component + Sync + Send
{
    type Item = &'a T;

    #[inline]
    fn drive_unindexed<C>(self, consumer: C) -> C::Result
        where C: UnindexedConsumer<Self::Item>
    {
        let producer = ReadProducer {
            inner: self.inner,
            mask: self.mask,
            start: 0,
            end: self.est_size,
        };
        bridge_unindexed(producer, consumer)
    }
}

struct ReadProducer<'a, T: Component> {
    inner: &'a Read<'a, T>,
    mask: &'a EntityMask,
    start: usize,
    end: usize,
}

impl <'a, T> UnindexedProducer for ReadProducer<'a, T>
    where T: Component + Sync + Send
{
    type Item = &'a T;

    #[inline]
    fn split(self) -> (Self, Option<Self>) {
        let diff = self.end - self.start;
        if diff <= 2 {
            (self, None)
        } else {
            let mid = diff / 2;
            (
                ReadProducer {
                    inner: self.inner,
                    mask: self.mask,
                    start: self.start,
                    end: self.start + mid,
                },
                Some(ReadProducer {
                    inner: self.inner,
                    mask: self.mask,
                    start: self.start + mid,
                    end: self.end,
                })
            )
        }
    }

    fn fold_with<F>(mut self, mut folder: F) -> F
        where F: Folder<Self::Item>
    {
        while self.start != self.end && !folder.full() {
            if self.mask.mask.get(self.start) {
                let storage = unsafe { &*self.inner.storage };
                let c = unsafe { storage.get_unchecked_component(self.start as u32) };
                folder = folder.consume(c);
            }
            self.start += 1;
        }
        folder
    }
}



/// Parallel iterator over components
pub struct WriteParIter<'a, T: Component> {
    pub(crate) inner: &'a Write<'a, T>,
    pub(crate) est_size: usize,
    pub(crate) mask: &'a EntityMask,
}
unsafe impl <'a, T> Send for WriteParIter<'a, T>
    where T: Component + Send + Sync {}
unsafe impl <'a, T> Sync for WriteParIter<'a, T>
    where T: Component + Send + Sync {}

impl <'a, T> ParallelIterator for WriteParIter<'a, T>
    where T: Component + Sync + Send
{
    type Item = &'a mut T;

    #[inline]
    fn drive_unindexed<C>(self, consumer: C) -> C::Result
        where C: UnindexedConsumer<Self::Item>
    {
        let producer = WriteProducer {
            inner: self.inner,
            mask: self.mask,
            start: 0,
            end: self.est_size,
        };
        bridge_unindexed(producer, consumer)
    }
}

struct WriteProducer<'a, T: Component> {
    inner: &'a Write<'a, T>,
    mask: &'a EntityMask,
    start: usize,
    end: usize,
}
unsafe impl <'a, T> Send for WriteProducer<'a, T>
    where T: Component + Send + Sync {}
unsafe impl <'a, T> Sync for WriteProducer<'a, T>
    where T: Component + Send + Sync {}

impl <'a, T> UnindexedProducer for WriteProducer<'a, T>
    where T: Component + Sync + Send
{
    type Item = &'a mut T;

    #[inline]
    fn split(self) -> (Self, Option<Self>) {
        let diff = self.end - self.start;
        if diff <= 2 {
            (self, None)
        } else {
            let mid = diff / 2;
            (
                WriteProducer {
                    inner: self.inner,
                    mask: self.mask,
                    start: self.start,
                    end: self.start + mid,
                },
                Some(WriteProducer {
                    inner: self.inner,
                    mask: self.mask,
                    start: self.start + mid,
                    end: self.end,
                })
            )
        }
    }

    fn fold_with<F>(mut self, mut folder: F) -> F
        where F: Folder<Self::Item>
    {
        while self.start != self.end && !folder.full() {
            if self.mask.mask.get(self.start) {
                let storage = unsafe { &mut *self.inner.storage };
                let c = unsafe { storage.get_unchecked_component_mut(self.start as u32) };
                folder = folder.consume(c);
            }
            self.start += 1;
        }
        folder
    }
}