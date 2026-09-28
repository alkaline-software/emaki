// The app icon, drawn on a canvas: an emaki, a handscroll unrolled from
// right to left. The open stretch shows a conversation as coloured blocks,
// earlier rounds run off the left edge, and the roll on the right still
// holds more. Same palette as the window (themes/scribe.json).
//
//   node draw.js <out-dir>   writes icon-1024.png, the smaller PNGs, icon.ico
//                            and an icon.iconset/ for iconutil.
const { createCanvas } = require('@napi-rs/canvas');
const fs = require('fs'), path = require('path');

const S = 1024;
const ink = '#262624', paper = '#FFFDF7', line = '#E9E4D6';
const terracotta = '#D97757', blue = '#5B7FB8', green = '#5F9E6E', gold = '#E2B84B';

function draw(size) {
  const c = createCanvas(size, size), x = c.getContext('2d');
  const k = size / S;
  x.scale(k, k);
  const rr = (x0, y0, w, h, r) => { x.beginPath(); x.roundRect(x0, y0, w, h, r); };
  const block = (bx, by, bw, bh, col, r = 18) => { rr(bx, by, bw, bh, r); x.fillStyle = col; x.fill(); };

  // Plate: terracotta, the accent the window uses, warm at the top.
  const P = 100, PW = S - 2 * P;
  x.save();
  x.shadowColor = 'rgba(0,0,0,0.20)'; x.shadowBlur = 40 ; x.shadowOffsetY = 18;
  rr(P, P, PW, PW, 184); x.fillStyle = terracotta; x.fill();
  x.restore();
  const g = x.createLinearGradient(0, P, 0, P + PW);
  g.addColorStop(0, '#E98D6C'); g.addColorStop(1, '#C8623F');
  rr(P, P, PW, PW, 184); x.fillStyle = g; x.fill();

  x.save();
  rr(P, P, PW, PW, 184); x.clip();

  // The open sheet, from beyond the plate's left edge to the roll.
  const top = 300, h = 424, bottom = top + h;
  const rollX = 690, rollW = 150, sheetL = P - 80;
  x.save(); x.shadowColor = 'rgba(38,38,36,0.28)'; x.shadowBlur = 30; x.shadowOffsetY = 12;
  x.fillStyle = paper; x.fillRect(sheetL, top, rollX + 40 - sheetL, h); x.restore();
  x.strokeStyle = line; x.lineWidth = 4;
  x.beginPath(); x.moveTo(sheetL, top + 3); x.lineTo(rollX, top + 3); x.moveTo(sheetL, bottom - 3); x.lineTo(rollX, bottom - 3); x.stroke();

  // The conversation: prompts on the right in terracotta, replies in blue,
  // a tool line in green, and the round still being written in gold.
  const cx0 = P + 78, cw = rollX - cx0 - 60;
  block(cx0 + cw - 270, top + 56, 270, 70, terracotta, 26);
  block(cx0, top + 156, 340, 28, blue, 14); block(cx0, top + 200, 240, 28, blue, 14);
  block(cx0, top + 244, 130, 28, green, 14);
  block(cx0 + cw - 200, top + 300, 200, 70, terracotta, 26);
  block(cx0, top + 396, 210, 28, gold, 14);
  // Earlier rounds, running off the left edge.
  x.globalAlpha = 0.30;
  block(sheetL, top + 56, P + 40 - sheetL, 70, terracotta, 26);
  block(sheetL, top + 156, P + 8 - sheetL, 28, blue, 14);
  block(sheetL, top + 300, P + 20 - sheetL, 70, terracotta, 26);
  x.globalAlpha = 1;

  // The sheet curling into the roll: a shaded crease.
  const crease = x.createLinearGradient(rollX - 60, 0, rollX + 10, 0);
  crease.addColorStop(0, 'rgba(38,38,36,0)'); crease.addColorStop(1, 'rgba(38,38,36,0.22)');
  x.fillStyle = crease; x.fillRect(rollX - 60, top, 70, h);

  // The roll: a cylinder, lit from the left, with staves top and bottom.
  const rg = x.createLinearGradient(rollX, 0, rollX + rollW, 0);
  rg.addColorStop(0, '#D8D2C2'); rg.addColorStop(0.28, '#FFFDF7'); rg.addColorStop(0.62, '#F4F0E4'); rg.addColorStop(1, '#C9C2AE');
  x.save(); x.shadowColor = 'rgba(38,38,36,0.30)'; x.shadowBlur = 34; x.shadowOffsetX = -8;
  rr(rollX, top - 30, rollW, h + 60, 75); x.fillStyle = rg; x.fill(); x.restore();
  x.strokeStyle = 'rgba(38,38,36,0.10)'; x.lineWidth = 6;
  for (let i = 0; i < 3; i++) { x.beginPath(); x.moveTo(rollX + 40 + i * 30, top - 12); x.lineTo(rollX + 40 + i * 30, bottom + 12); x.stroke(); }
  block(rollX - 8, top - 70, rollW + 16, 48, ink, 18);
  block(rollX - 8, bottom + 22, rollW + 16, 48, ink, 18);
  // A blue ribbon around the roll, the tie that keeps it.
  block(rollX + 16, top + h / 2 - 26, rollW - 32, 52, blue, 14);
  x.restore();
  return c;
}

const out = process.argv[2] || '.';
fs.mkdirSync(out, { recursive: true });
const big = draw(1024);
fs.writeFileSync(path.join(out, 'icon-1024.png'), big.toBuffer('image/png'));
// Each size is drawn at that size, not resampled, so edges stay crisp.
const iconset = path.join(out, 'icon.iconset');
fs.mkdirSync(iconset, { recursive: true });
for (const [name, size] of [['16x16', 16], ['16x16@2x', 32], ['32x32', 32], ['32x32@2x', 64], ['128x128', 128], ['128x128@2x', 256], ['256x256', 256], ['256x256@2x', 512], ['512x512', 512], ['512x512@2x', 1024]]) {
  fs.writeFileSync(path.join(iconset, `icon_${name}.png`), draw(size).toBuffer('image/png'));
}
for (const size of [32, 128, 256, 512]) {
  fs.writeFileSync(path.join(out, `icon-${size}.png`), draw(size).toBuffer('image/png'));
}
// ICO: a directory of PNG-compressed entries, which Windows reads since Vista.
const sizes = [16, 24, 32, 48, 64, 128, 256];
const pngs = sizes.map(s => draw(s).toBuffer('image/png'));
const header = Buffer.alloc(6); header.writeUInt16LE(0, 0); header.writeUInt16LE(1, 2); header.writeUInt16LE(sizes.length, 4);
const dir = Buffer.alloc(16 * sizes.length);
let offset = 6 + dir.length;
sizes.forEach((s, i) => {
  const e = i * 16;
  dir.writeUInt8(s === 256 ? 0 : s, e); dir.writeUInt8(s === 256 ? 0 : s, e + 1);
  dir.writeUInt8(0, e + 2); dir.writeUInt8(0, e + 3);
  dir.writeUInt16LE(1, e + 4); dir.writeUInt16LE(32, e + 6);
  dir.writeUInt32LE(pngs[i].length, e + 8); dir.writeUInt32LE(offset, e + 12);
  offset += pngs[i].length;
});
fs.writeFileSync(path.join(out, 'icon.ico'), Buffer.concat([header, dir, ...pngs]));
console.log('icon written to', out);
