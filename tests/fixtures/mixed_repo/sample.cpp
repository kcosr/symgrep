// Mixed-language C++ fixture for symgrep symbol extraction tests.

namespace util {

struct Widget {
    int value;

    int increment(int delta) {
        return value + delta;
    }
};

class Greeter {
public:
    void greet();
};

void Greeter::greet() {
}

} // namespace util

int add(int a, int b) {
    return a + b;
}

