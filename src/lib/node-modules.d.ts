// version.test.ts reads package files. This repo does not depend on @types/node.
declare module "node:fs" {
  export function readFileSync(path: string, encoding: "utf8"): string;
}
