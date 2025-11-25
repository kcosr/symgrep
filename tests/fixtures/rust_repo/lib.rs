//! Simple Rust fixture for symgrep symbol extraction tests.

pub mod my_mod {
    pub struct Widget {
        pub value: i32,
    }

    impl Widget {
        pub fn new(value: i32) -> Self {
            Widget { value }
        }

        pub fn increment(&self, delta: i32) -> i32 {
            self.value + delta
        }
    }

    pub trait Greeter {
        fn greet(&self);
    }

    pub fn add(a: i32, b: i32) -> i32 {
        a + b
    }
}

/// Adds two integers with a doc comment.
/// Used to exercise Rust comment extraction.
pub fn add_with_doc(a: i32, b: i32) -> i32 {
    a + b
}

pub mod deep {
    pub mod level1 {
        pub mod level2 {
            pub struct DeepWidget {
                pub value: i32,
            }

            impl DeepWidget {
                pub fn depth(&self) -> i32 {
                    self.value
                }
            }
        }
    }
}
