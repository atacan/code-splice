export function keep(): string {
  return "kept";
}
export interface Named {
  name: string;
}
export class Person implements Named {
  constructor(public readonly name: string) {}
}
export namespace Person {
  export function guest(): Person {
    return new Person("Guest");
  }
}
export function formatGreeting(named: Named): string {
  return `Hello, ${named.name}!`;
}
