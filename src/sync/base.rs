trait Mutex {
    type Guard;

    fn lock(&self) -> Self::Guard;
}

trait Guard {}
