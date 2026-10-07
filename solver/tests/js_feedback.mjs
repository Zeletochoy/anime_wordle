import { readFileSync } from "node:fs";
import { buildResult } from "../../src/utils/gameLogic.js";

const db = JSON.parse(readFileSync(process.argv[2], "utf8"));
const out = [];
for (const g of db) for (const t of db) out.push(buildResult(g, t).cells.map((c) => c.status).join(","));
process.stdout.write(out.join("\n") + "\n");
