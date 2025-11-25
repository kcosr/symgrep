export function bar(): void {}

export function baz(): void {}

export function foo(): void {
  bar();
  baz();
}

export function qux(): void {
  foo();
}

