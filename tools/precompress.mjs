// Writes <file>.br (brotli q11) and <file>.gz (gzip -9) next to each text asset in a dir (doc 15 §4).
import { readdirSync, readFileSync, writeFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { brotliCompressSync, gzipSync, constants } from "node:zlib";

const TEXT = /\.(html|js|css|svg|json|map|txt|webmanifest)$/;
const dir = process.argv[2];
if (!dir) throw new Error("usage: precompress.mjs <dir>");

const walk = (d) =>
  readdirSync(d).flatMap((n) => {
    const p = join(d, n);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });

for (const f of walk(dir).filter((f) => TEXT.test(f))) {
  const raw = readFileSync(f);
  const br = brotliCompressSync(raw, {
    params: { [constants.BROTLI_PARAM_QUALITY]: 11, [constants.BROTLI_PARAM_SIZE_HINT]: raw.length },
  });
  const gz = gzipSync(raw, { level: 9 });
  // Skip when compression does not pay (tiny files).
  if (br.length < raw.length) writeFileSync(`${f}.br`, br);
  if (gz.length < raw.length) writeFileSync(`${f}.gz`, gz);
}
