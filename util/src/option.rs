pub trait OptionFlip<S, E> {}

impl<S, E> OptionFlip<S, E> for Option<Result<S, E>> {}
