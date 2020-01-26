
pub trait VecExt<T> {
    fn groups<F>(&self, cmp: F) -> GroupIter<T, F>
        where F: Fn(&T, &T) -> bool;
}

impl <T> VecExt<T> for [T] {
    fn groups<F>(&self, cmp: F) -> GroupIter<T, F>
        where F: Fn(&T, &T) -> bool
    {
        GroupIter {
            data: self,
            cmp,
            offset: 0,
        }
    }
}

pub struct GroupIter<'a, T: 'a, F> {
    data: &'a [T],
    cmp: F,
    offset: usize,
}

impl <'a, T: 'a, F> Iterator for GroupIter<'a, T, F>
        where F: Fn(&T, &T) -> bool
{
    type Item = &'a [T];

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(first) = self.data.get(self.offset) {
            let mut len = 1;
            while let Some(b) = self.data.get(self.offset + len) {
                if !(self.cmp)(first, b) {
                    break;
                }
                len += 1;
            }
            let out = &self.data[self.offset .. self.offset + len];
            self.offset += len;
            Some(out)
        } else {
            None
        }
    }
}

pub trait IterExt: Iterator {
    fn flat_join<U, F>(self, f: F) -> FlatJoin<Self, U, F>
        where F: FnMut(&Self::Item) -> U,
              U: IntoIterator,
              Self: Sized,
              Self::Item: Clone,
    {
        FlatJoin {
            iter: self,
            func: f,
            front: None,
        }
    }
}

pub struct FlatJoin<I, U, F>
    where U: IntoIterator,
          I: Iterator
{
    iter: I,
    func: F,
    front: Option<(I::Item, U::IntoIter)>,
}

impl <I, U, F> Iterator for FlatJoin<I, U, F>
    where F: FnMut(&I::Item) -> U,
            U: IntoIterator,
            I: Iterator,
            I::Item: Clone,
{
    type Item = (I::Item, U::Item);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut front) = self.front {
                if let Some(val) = front.1.next() {
                    return Some((front.0.clone(), val));
                }
            }
            let cur = self.iter.next()?;
            let front = (self.func)(&cur).into_iter();
            self.front = Some((cur, front));
        }
    }
}

#[test]
fn flat_join_test() {
    let mut data = vec![1, 2, 3]
        .into_iter()
        .flat_join(|v| vec![*v, 2 * v, 3 * v]);

    assert_eq!(data.next(), Some((1, 1)));
    assert_eq!(data.next(), Some((1, 2)));
    assert_eq!(data.next(), Some((1, 3)));
    assert_eq!(data.next(), Some((2, 2)));
    assert_eq!(data.next(), Some((2, 4)));
    assert_eq!(data.next(), Some((2, 6)));
    assert_eq!(data.next(), Some((3, 3)));
    assert_eq!(data.next(), Some((3, 6)));
    assert_eq!(data.next(), Some((3, 9)));
}

impl <I> IterExt for I
    where I: Iterator
{

}