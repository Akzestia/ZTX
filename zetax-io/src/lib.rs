use tokio::sync::oneshot;

pub fn ray<T, F>(job: F) -> impl std::future::Future<Output = T> + Send
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    let (tx, rx) = oneshot::channel::<T>();
    rayon::spawn(move || {
        let res = job();
        let _ = tx.send(res);
    });
    async move { rx.await.expect("rayon task panicked or dropped") }
}

#[macro_export]
macro_rules! ray {
    ($($tt:tt)*) => {{
        $crate::ray(move || { $($tt)* })
    }};
}

#[derive(Debug)]
pub enum RayErr {
    Panicked,
    Canceled,
}

pub fn ray_result<T, F>(job: F) -> impl std::future::Future<Output = Result<T, RayErr>> + Send
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + std::panic::UnwindSafe + 'static,
{
    let (tx, rx) = oneshot::channel::<Result<T, RayErr>>();
    rayon::spawn(move || {
        let res = std::panic::catch_unwind(job).map_err(|_| RayErr::Panicked);
        let _ = tx.send(res);
    });
    async move { rx.await.unwrap_or(Err(RayErr::Canceled)) }
}

#[macro_export]
macro_rules! ray_result {
    ($($tt:tt)*) => {{
        $crate::ray_result(move || { $($tt)* })
    }};
}

#[macro_export]
macro_rules! par_iter {
    ($iter:expr => $body:expr) => {{
        use ::rayon::prelude::*;
        ($iter).into_par_iter().map($body).collect::<Vec<_>>()
    }};
}

#[macro_export]
macro_rules! par_for {
    ($iter:expr => $body:expr) => {{
        use ::rayon::prelude::*;
        ($iter).into_par_iter().for_each($body)
    }};
}

#[macro_export]
macro_rules! par_chunks {
    ($slice:expr, $size:expr => $body:expr) => {{
        use ::rayon::prelude::*;
        ($slice).par_chunks($size).for_each($body)
    }};
}

#[macro_export]
macro_rules! par_chunks_mut {
    ($slice:expr, $size:expr => $body:expr) => {{
        use ::rayon::prelude::*;
        ($slice).par_chunks_mut($size).for_each($body)
    }};
}

#[macro_export]
macro_rules! par_chunks_map {
    ($slice:expr, $size:expr => $body:expr) => {{
        use ::rayon::prelude::*;
        ($slice).par_chunks($size).map($body).collect::<Vec<_>>()
    }};
}

#[macro_export]
macro_rules! par_sort {
    ($vec:expr) => {{
        use ::rayon::prelude::*;
        ($vec).par_sort()
    }};
    ($vec:expr, by $cmp:expr) => {{
        use ::rayon::prelude::*;
        ($vec).par_sort_by($cmp)
    }};
    ($vec:expr, key $key:expr) => {{
        use ::rayon::prelude::*;
        ($vec).par_sort_by_key($key)
    }};
}

#[macro_export]
macro_rules! par_join {
    ($a:expr, $b:expr) => {{ ::rayon::join(|| $a, || $b) }};
}

#[macro_export]
macro_rules! par_async {
    ($iter:expr => $body:expr) => {{
        $crate::ray(move || {
            use ::rayon::prelude::*;
            ($iter).into_par_iter().map($body).collect::<Vec<_>>()
        })
    }};
}

#[macro_export]
macro_rules! par_try_iter {
    ($iter:expr => $body:expr) => {{
        use ::rayon::prelude::*;
        ($iter)
            .into_par_iter()
            .map($body)
            .collect::<Result<Vec<_>, _>>()
    }};
}

#[macro_export]
macro_rules! par_try_async {
    ($iter:expr => $body:expr) => {{
        $crate::ray(move || {
            use ::rayon::prelude::*;
            ($iter)
                .into_par_iter()
                .map($body)
                .collect::<Result<Vec<_>, _>>()
        })
    }};
}

#[macro_export]
macro_rules! par_option_iter {
    ($iter:expr => $body:expr) => {{
        use ::rayon::prelude::*;
        ($iter)
            .into_par_iter()
            .map($body)
            .collect::<Option<Vec<_>>>()
    }};
}

#[macro_export]
macro_rules! par_option_async {
    ($iter:expr => $body:expr) => {{
        $crate::ray(move || {
            use ::rayon::prelude::*;
            ($iter)
                .into_par_iter()
                .map($body)
                .collect::<Option<Vec<_>>>()
        })
    }};
}

#[macro_export]
macro_rules! par_any {
    ($iter:expr => $pred:expr) => {{
        use ::rayon::prelude::*;
        ($iter).into_par_iter().any($pred)
    }};
}

#[macro_export]
macro_rules! par_all {
    ($iter:expr => $pred:expr) => {{
        use ::rayon::prelude::*;
        ($iter).into_par_iter().all($pred)
    }};
}

#[macro_export]
macro_rules! par_find_any {
    ($iter:expr => $pred:expr) => {{
        use ::rayon::prelude::*;
        ($iter).into_par_iter().find_any(|x| $pred(x))
    }};
}
