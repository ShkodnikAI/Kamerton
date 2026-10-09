// Н2-04 · проверка пейсера скриптом против эталонной раскадки.
// Запуск из корня репо: node self-host/miniapp/test/pacer-test.mjs
//
// Источник математики — сама страница self-host/miniapp/index.html
// (маркеры PacerEngine-START/END держат единственную копию), эталон —
// аналитическая форма, заявленная в комментарии движка: период 60/T мс,
// равные половины вдоха/выдоха, масштаб 0.10–1.00 синусоидой,
// старт из середины 0.55 на вдохе, фаза от абсолютного времени шага
// (без накопления дрейфа). Три контрольных темпа наряда: 4.5, 5.75, 7.0.
import { readFileSync } from "node:fs";

const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
const m = html.match(/\/\/ PacerEngine-START[\s\S]*?\/\/ PacerEngine-END/);
if (!m) throw new Error("PacerEngine не найден в index.html");
const pacerState = new Function(m[0] + "\nreturn pacerState;")();

let fails = 0;
function ok(cond, label) {
  if (!cond) { fails += 1; console.error("FAIL " + label); }
}

const TEMPOS = [4.5, 5.75, 7.0]; // три контрольных темпа наряда Н2-04
for (const T of TEMPOS) {
  const period = 60000 / T;
  ok(Math.abs(pacerState(T, 0).periodMs - period) < 1e-9, `T=${T}: период 60/T`);

  // Эталонная раскадровка — четверти периода (аналитическая синусоида).
  const ref = [[0, 0.55], [period * 0.25, 1.0], [period * 0.5, 0.55], [period * 0.75, 0.1]];
  for (const [t, v] of ref) {
    const st = pacerState(T, t);
    ok(Math.abs(st.scale - v) < 1e-9, `T=${T}: t=${t.toFixed(1)}мс scale ${st.scale.toFixed(6)} != ${v}`);
  }

  // Направление: первая половина цикла — вдох, вторая — выдох.
  ok(pacerState(T, period * 0.25).dir === "вдох", `T=${T}: вдох в первой половине`);
  ok(pacerState(T, period * 0.75).dir === "выдох", `T=${T}: выдох во второй половине`);

  // Плавность: поквартальная монотонность раскадки (50 точек на четверть),
  // паттерн синусоиды: вверх, вниз, вниз, вверх.
  const dirs = [1, -1, -1, 1];
  for (let q = 0; q < 4; q++) {
    let prev = pacerState(T, (period * q) / 4).scale;
    for (let i = 1; i <= 50; i++) {
      const t = (period * (q + i / 50)) / 4;
      const sc = pacerState(T, t).scale;
      if (dirs[q] > 0) ok(sc >= prev - 1e-12, `T=${T}: рост в четверти ${q} на t=${t.toFixed(1)}`);
      else ok(sc <= prev + 1e-12, `T=${T}: спад в четверти ${q} на t=${t.toFixed(1)}`);
      prev = sc;
    }
  }

  // Шаг завершается на t=120000 мс: фаза без дрейфа, число циклов T*2.
  // Дистанция ЦИКЛИЧЕСКАЯ (граница цикла в float представляется и как
  // 0.0, и как 1.0−ε — например 4.5×2 = 9.000000000000001 цикла):
  const expectedPhase = ((T * 2) % 1);
  const stEnd = pacerState(T, 120000);
  const d = Math.abs(stEnd.phase - expectedPhase);
  const circ = Math.min(d, 1 - d);
  ok(circ < 1e-9, `T=${T}: фаза на конце 2-минутного шага = ${expectedPhase} (циклически), дрейф ${circ.toExponential(2)}`);
}

// Чистая арифметика диапазона темпа (клампы UI 4.5–7.0, шаг 0.25).
ok(Math.round((4.5 + 0.25) * 100) / 100 === 4.75, "шаг вверх 4.5→4.75");
ok(Math.round((5.5 + 0.25) * 100) / 100 === 5.75, "шаг вверх 5.5→5.75");
ok(Math.min(7.0, Math.round((6.75 + 0.25) * 100) / 100) === 7.0, "кламп верха 7.0");
ok(Math.max(4.5, Math.round((4.5 - 0.25) * 100) / 100) === 4.5, "кламп низа 4.5");
ok(Math.max(4.5, Math.round((5.0 - 0.25) * 100) / 100) === 4.75, "шаг вниз 5.0→4.75");

if (fails > 0) { console.error(`${fails} провал(ов)`); process.exit(1); }
console.log("PACER-TEST: ALL GREEN — 3 контрольных темпа держат эталонную раскадку (4.5, 5.75, 7.0 вдох/мин)");
