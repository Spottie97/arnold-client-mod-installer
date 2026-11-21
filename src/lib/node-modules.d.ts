// Tests read package files. This repo does not depend on @types/node.
declare module "node:fs" {
  export function readFileSync(path: string, encoding: "utf8"): string;
  export function readFileSync(path: string): Uint8Array;
  export function readdirSync(path: string): string[];
}

declare module "node:path" {
  function join(...parts: string[]): string;
  export default join;
  export { join };
}

declare module "node:crypto" {
  interface Hash {
    update(data: Uint8Array): Hash;
    digest(encoding: "hex"): string;
  }
  export function createHash(algorithm: string): Hash;
}
