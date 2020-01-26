

pub trait ResultExt<O, E> {
    fn and_then_into<F, V, R>(self, f: F) -> Result<V, R>
        where F: FnOnce(O) -> Result<V, R>,
                E: Into<R>;
}

impl <O, E> ResultExt<O, E> for Result<O, E> {
    fn and_then_into<F, V, R>(self, f: F) -> Result<V, R>
        where F: FnOnce(O) -> Result<V, R>,
                E: Into<R> {
        match self {
            Ok(val) => f(val),
            Err(e) => Err(e.into()),
        }
    }
}
