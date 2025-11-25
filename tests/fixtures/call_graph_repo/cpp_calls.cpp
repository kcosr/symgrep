namespace cg {

void bar() {}

void baz() {}

void foo() {
    bar();
    baz();
}

void qux() {
    foo();
}

} // namespace cg

