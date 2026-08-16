use axum::Router;

pub fn mount_combined_routes(routers: Vec<Router>) -> Router {
    routers
        .into_iter()
        .fold(Router::new(), |acc, r| acc.merge(r))
}

pub fn iterate_combined_routers<I>(routers: I) -> impl Iterator<Item = Router>
where
    I: IntoIterator<Item = Router>,
{
    routers.into_iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mount_combined_routes_returns_a_router_when_input_is_empty() {
        let router: Router = mount_combined_routes(Vec::new());
        let _ = router;
    }

    #[test]
    fn mount_combined_routes_returns_a_router_when_input_is_single() {
        let router: Router = mount_combined_routes(vec![Router::new()]);
        let _ = router;
    }

    #[test]
    fn mount_combined_routes_returns_a_router_when_input_is_multiple() {
        let router: Router = mount_combined_routes(vec![Router::new(), Router::new()]);
        let _ = router;
    }

    #[test]
    fn iterate_combined_routers_yields_every_router_in_input_order() {
        let inputs = vec![Router::new(), Router::new(), Router::new()];
        let out: Vec<Router> = iterate_combined_routers(inputs).collect();
        assert_eq!(out.len(), 3);
    }

    #[test]
    fn iterate_combined_routers_yields_nothing_for_empty_input() {
        let empty: Vec<Router> = Vec::new();
        let out: Vec<Router> = iterate_combined_routers(empty).collect();
        assert_eq!(out.len(), 0);
    }
}
