// Н2-06 · проверка сборки RR-сегментов протокола перебора.
// Запуск из корня репо: node self-host/miniapp/test/sweep-test.mjs
//
// Источник — сама страница self-host/miniapp/index.html (маркеры
// SweepEngine-START/END держат единственную копию сборщика отметок).
// Критерии наряда: отметка шага ВСЕГДА несёт поле rr (сервер отличает
// отметку Н2-06 от старого формата по ключу); брак QC едет громко в
// rr_reason, протокол не умирает; step_marks полного протокола (11 шагов
// с реалистичным RR) помещаются в лимит маршрута 32KiB; финальное
// закрытие сессии несёт final=1 — только оно запускает анализ на сервере.
import { readFileSync } from "node:fs";

const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
const m = html.match(/\/\/ SweepEngine-START[\s\S]*?\/\/ SweepEngine-END/);
if (!m) throw new Error("SweepEngine не найден в index.html");
// Stubs вместо ppg-движка: stepMark вызывается с подставленными исходами QC.
const engine = new Function(`
  var ppgExtractRR = function (samples) { return samples[0].ex; };
  var ppgQualityRefusal = function (ex) { return ex.refusal || ""; };
  var ppgStream = null, ppgSampling = false, ppgStart_ = 0;
  var stepNo = 0, tempo = 0, stepSamples = [], stepPpgOn = false;
  ${m[0]}
  return { stepMark, set: function (s) { stepNo = s.stepNo; tempo = s.tempo; stepSamples = s.stepSamples; stepPpgOn = s.stepPpgOn; } };
`)();

let fails = 0;
function ok(cond, label) {
  if (!cond) { fails += 1; console.error("FAIL " + label); }
}

// Сборка RR-сегмента протокола: beats уд/мин на 2-минутном шаге.
function rrSegment(bpm, seconds) {
  const rr = [];
  const mean = 60000 / bpm;
  for (let t = 0; t < seconds * 1000; t += mean) { rr.push(Math.round(mean)); }
  return rr;
}

// 1. Отметка без записи пульса: поле rr присутствует (ключ всегда).
{
  const rec = engine.stepMark(); // stepPpgOn=false по умолчанию
  ok(Object.prototype.hasOwnProperty.call(rec, "rr"), "отметка несёт ключ rr всегда");
  ok(Array.isArray(rec.rr) && rec.rr.length === 0, "rr пуст без записи пульса");
  ok(rec.rr_reason === "", "rr_reason пуст без записи пульса");
}
// 2. Годный сегмент: rr едет в отметку, rr_reason пуст.
{
  const good = { rr: rrSegment(60, 120), epochsTotal: 12, epochsRejected: 0 };
  engine.set({ stepNo: 3, tempo: 5.5, stepSamples: [{ ex: good }], stepPpgOn: true });
  const rec = engine.stepMark();
  ok(rec.step === 3 && rec.tempo_bpm === 5.5, "отметка несёт шаг и темп");
  ok(rec.rr.length === good.rr.length, "годный сегмент поехал в rr целиком");
  ok(rec.rr_reason === "", "rr_reason пуст у годного сегмента");
}

// 3. Брак QC: rr пуст, отказ едет громко в rr_reason, ключи на месте.
{
  const bad = { rr: [900], epochsTotal: 12, epochsRejected: 8, refusal: "брак: отбраковано эпох 8 из 12 — сигнала пульса нет" };
  engine.set({ stepNo: 4, tempo: 6.0, stepSamples: [{ ex: bad }], stepPpgOn: true });
  const rec = engine.stepMark();
  ok(rec.rr.length === 0, "бракованный сегмент не едет в rr");
  ok(rec.rr_reason.indexOf("брак") === 0, "отказ QC едет громко в rr_reason");
  ok(Object.prototype.hasOwnProperty.call(rec, "rr"), "ключ rr присутствует и у брака");
}

// 4. Лимит маршрута: step_marks полного протокола из 11 шагов — 32KiB.
{
  const tempos = [4.5, 4.75, 5.0, 5.25, 5.5, 5.75, 6.0, 6.25, 6.5, 6.75, 7.0];
  const marks = tempos.map((t, i) => ({ step: i + 1, tempo_bpm: t, rr: rrSegment(60, 120), rr_reason: "" }));
  const size = JSON.stringify(marks).length;
  ok(size < 32768, `полный протокол (60 уд/мин) в лимите 32KiB: ${size}`);
  // Экстремальный пульс 200 уд/мин — верхняя граница QC движка Н2-05.
  const marksFast = tempos.map((t, i) => ({ step: i + 1, tempo_bpm: t, rr: rrSegment(200, 120), rr_reason: "" }));
  const sizeFast = JSON.stringify(marksFast).length;
  ok(sizeFast < 32768, `полный протокол (200 уд/мин) в лимите 32KiB: ${sizeFast}`);
}

// 5. Финальное закрытие несёт final=1; инкрементальные — нет.
ok(/final: "1"/.test(html), "closeSession шлёт final=1");
ok(/openSession\('sweep'\)/.test(html), "кнопка протокола перебора на странице");
ok(/ppg-during-step/.test(html), "переключатель пульса на шагах на странице");

if (fails > 0) { console.error(`\n${fails} провалов`); process.exit(1); }
console.log("sweep-test: ALL GREEN");
