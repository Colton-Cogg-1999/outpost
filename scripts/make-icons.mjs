// Renders OUTPOST's original pixel-art icon (a rook-shaped research tower with an
// antenna) to PNG and ICO without any image libraries. Run: npm run icons
import { deflateSync } from 'node:zlib';
import { mkdirSync, writeFileSync } from 'node:fs';

const SPRITE = [
  '...............',
  '.......#.......',
  '.......#.......',
  '......#+#......',
  '.......#.......',
  '..##.##.##.##..',
  '..###########..',
  '...#########...',
  '....#######....',
  '....##+.+##....',
  '....#######....',
  '....#######....',
  '...#########...',
  '..###########..',
  '.#############.',
];
const COLORS = { '#': [150, 214, 160, 255], '+': [70, 120, 84, 255] };
const BG = [9, 15, 11, 235];
const EDGE = [62, 104, 74, 255];

function crc32(buf) {
  let c, crc = 0xffffffff;
  for (let n = 0; n < buf.length; n++) {
    c = (crc ^ buf[n]) & 0xff;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    crc = (crc >>> 8) ^ c;
  }
  return (crc ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
  const td = Buffer.concat([Buffer.from(type), data]);
  const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(td));
  return Buffer.concat([len, td, crc]);
}
function png(size) {
  const px = Buffer.alloc(size * size * 4);
  const r = Math.round(size * 0.18);
  const inside = (x, y) => {
    const cx = Math.min(Math.max(x, r), size - 1 - r), cy = Math.min(Math.max(y, r), size - 1 - r);
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r;
  };
  const scale = Math.floor((size * 0.8) / SPRITE.length);
  const off = Math.floor((size - scale * SPRITE.length) / 2);
  for (let y = 0; y < size; y++) for (let x = 0; x < size; x++) {
    let c = [0, 0, 0, 0];
    if (inside(x, y)) {
      c = BG;
      const edge = !inside(x - 1, y) || !inside(x + 1, y) || !inside(x, y - 1) || !inside(x, y + 1);
      if (edge && size >= 64) c = EDGE;
      const sx = Math.floor((x - off) / scale), sy = Math.floor((y - off) / scale);
      const ch = SPRITE[sy]?.[sx];
      if (x >= off && y >= off && ch && COLORS[ch]) c = COLORS[ch];
    }
    px.set(c, (y * size + x) * 4);
  }
  const raw = Buffer.alloc(size * (size * 4 + 1));
  for (let y = 0; y < size; y++) px.copy(raw, y * (size * 4 + 1) + 1, y * size * 4, (y + 1) * size * 4);
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(size, 0); ihdr.writeUInt32BE(size, 4);
  ihdr[8] = 8; ihdr[9] = 6;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', ihdr), chunk('IDAT', deflateSync(raw, { level: 9 })), chunk('IEND', Buffer.alloc(0))]);
}
function ico(pngs) {
  const header = Buffer.alloc(6); header.writeUInt16LE(1, 2); header.writeUInt16LE(pngs.length, 4);
  let offset = 6 + 16 * pngs.length;
  const entries = pngs.map(([size, data]) => {
    const e = Buffer.alloc(16);
    e[0] = size >= 256 ? 0 : size; e[1] = size >= 256 ? 0 : size;
    e.writeUInt16LE(1, 4); e.writeUInt16LE(32, 6);
    e.writeUInt32LE(data.length, 8); e.writeUInt32LE(offset, 12);
    offset += data.length;
    return e;
  });
  return Buffer.concat([header, ...entries, ...pngs.map(([, d]) => d)]);
}
const dir = new URL('../src-tauri/icons/', import.meta.url);
mkdirSync(dir, { recursive: true });
const out = { '32x32.png': 32, '64x64.png': 64, '128x128.png': 128, '128x128@2x.png': 256, 'icon.png': 512 };
for (const [name, size] of Object.entries(out)) writeFileSync(new URL(name, dir), png(size));
writeFileSync(new URL('icon.ico', dir), ico([[32, png(32)], [64, png(64)], [256, png(256)]]));
writeFileSync(new URL('../public/favicon.png', import.meta.url), png(64));
console.log('icons written');
