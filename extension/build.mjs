// Copies the static files next to tsc's output in dist/ and draws the
// icons (no image files in the repository, no extra packages).
import { cpSync, mkdirSync, writeFileSync } from "node:fs";
import { deflateSync } from "node:zlib";

const out = new URL("./dist/", import.meta.url);
cpSync(new URL("./static/", import.meta.url), out, { recursive: true });
mkdirSync(new URL("./icons/", out), { recursive: true });

const CRC = new Uint32Array(256).map((_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});
function crc32(buf) {
  let c = 0xffffffff;
  for (const b of buf) c = CRC[(c ^ b) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}

/** A rounded blue square with three rising bars, `size` pixels wide. */
function icon(size) {
  const stride = size * 4 + 1;
  const px = Buffer.alloc(size * stride);
  const r = size * 0.22;
  const bars = [
    [0.2, 0.36, 0.58],
    [0.42, 0.58, 0.4],
    [0.64, 0.8, 0.22],
  ];
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const cx = Math.min(x, size - 1 - x);
      const cy = Math.min(y, size - 1 - y);
      const inside = cx >= r || cy >= r || (r - cx) ** 2 + (r - cy) ** 2 <= r * r;
      if (!inside) continue;
      const u = (x + 0.5) / size;
      const v = (y + 0.5) / size;
      const bar = bars.some(([a, b, top]) => u >= a && u < b && v >= top && v < 0.8);
      const i = y * stride + 1 + x * 4;
      const [R, G, B] = bar ? [255, 255, 255] : [42, 120, 214];
      px[i] = R;
      px[i + 1] = G;
      px[i + 2] = B;
      px[i + 3] = 255;
    }
  }
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0);
  ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; // bits per channel
  ihdr[9] = 6; // RGBA
  return Buffer.concat([
    Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflateSync(px)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

for (const s of [16, 32, 48, 128]) writeFileSync(new URL(`./icons/${s}.png`, out), icon(s));
console.log("extension built in extension/dist (load it unpacked)");
