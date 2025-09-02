pub type Handler = fn();

pub struct Route {
    pub route: &'static str,
    pub handler: Handler,
}

inventory::collect!(Route);
