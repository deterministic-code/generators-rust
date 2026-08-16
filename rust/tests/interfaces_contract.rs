use deterministic::repositories::inmemory::{
    InMemoryCrudRepository, InMemoryRepository, InMemoryStandardRepository,
};
use deterministic::{CrudRepository, Repository, StandardCrudRepository};

fn _assert_repository<T: Repository + ?Sized>() {}
fn _assert_crud<T: CrudRepository + ?Sized>() {}
fn _assert_standard_crud<T: StandardCrudRepository + ?Sized>() {}

#[test]
fn in_memory_implements_traits() {
    _assert_repository::<InMemoryRepository>();
    _assert_repository::<InMemoryCrudRepository>();
    _assert_crud::<InMemoryCrudRepository>();
    _assert_repository::<InMemoryStandardRepository>();
    _assert_crud::<InMemoryStandardRepository>();
    _assert_standard_crud::<InMemoryStandardRepository>();
}

#[test]
fn dyn_objects_can_be_constructed() {
    let _r: Box<dyn Repository> = Box::new(InMemoryRepository::new());
    let _c: Box<dyn CrudRepository> = Box::new(InMemoryCrudRepository::new());
    let _s: Box<dyn StandardCrudRepository> = Box::new(InMemoryStandardRepository::new());
}
