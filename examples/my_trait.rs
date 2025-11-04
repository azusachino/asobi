#![allow(unused)]

use anyhow::Result;
use std::fmt::Debug;

#[tokio::main]
async fn main() {
    println!("wow, my trait!");
}

trait TraitA<T> {
    fn do_work();
}

struct AType<U> {
    a: U,
}

impl<T, U> TraitA<T> for AType<U>
where
    T: Debug,
    U: PartialEq,
{
    fn do_work() {}
}

trait TraitB {}

fn my_trait() -> Option<Box<dyn TraitB>> {
    None
}

fn do_it(x: impl TraitB) {}

fn do_it_v2(x: &dyn TraitB) {}

trait SafeTrait {
    fn foo(&self) {}
    fn foo_mut(&mut self) {}
    fn foo_box(self: Box<Self>) {}
}

trait NotObjectSafe {
    const CONST: i32 = 1; // 不能包含关联常量

    fn foo() {} // 不能包含这样的关联函数
    fn selfin(self); // 不能将Self所有权传入
    fn returns(&self) -> Self; // 不能返回Self
    fn typed<T>(&self, x: T) {} // 方法中不能有类型参数
}
