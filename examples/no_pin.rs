#![allow(unused)]

struct SelfReferential {
    data: String,
    self_pointer: *const String,
}

impl SelfReferential {
    fn new(data: String) -> Self {
        let mut sr = SelfReferential {
            data,
            self_pointer: std::ptr::null(),
        };
        sr.self_pointer = &sr.data as *const String;
        sr
    }

    fn print(&self) {
        unsafe {
            println!("{}", *self.self_pointer);
        }
    }
}

fn main() {
    let x = SelfReferential::new("hello".to_owned());
    x.print();
    let y = x;
    // segment fault?
    // unsafe precondition(s) violated: slice::from_raw_parts requires the pointer to be aligned and non-null, and the total size of the slice not to exceed `isize::MAX`
    y.print();
}
