// CSV schema test (backlog item 3, docs/plan/14): the column list in src/csv.js,
// the header of bench/devices/results.csv and the table in bench/devices/README.md
// must agree, and results.csv must only hold device rows.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import { COLUMNS, COLUMN_NAMES, csvEscape, csvHeader, csvRow, missingColumns } from "../src/csv.js";

const devices = new URL("../../devices/", import.meta.url);

/** Minimal RFC 4180 parser: returns rows of fields. */
function parseCsv(text) {
  const rows = [];
  let row = [];
  let field = "";
  let quoted = false;
  for (let i = 0; i < text.length; i++) {
    const c = text[i];
    if (quoted) {
      if (c === '"' && text[i + 1] === '"') {
        field += '"';
        i++;
      } else if (c === '"') quoted = false;
      else field += c;
    } else if (c === '"' && field === "") quoted = true;
    else if (c === ",") {
      row.push(field);
      field = "";
    } else if (c === "\n" || c === "\r") {
      if (c === "\r" && text[i + 1] === "\n") i++;
      row.push(field);
      rows.push(row);
      row = [];
      field = "";
    } else field += c;
  }
  if (field !== "" || row.length > 0) {
    row.push(field);
    rows.push(row);
  }
  return rows;
}

test("column names are unique snake_case identifiers with descriptions", () => {
  assert.equal(new Set(COLUMN_NAMES).size, COLUMN_NAMES.length);
  for (const [name, doc] of COLUMNS) {
    assert.match(name, /^[a-z][a-z0-9_]*$/, name);
    assert.ok(doc.length > 10, `${name} needs a description`);
  }
});

test("the columns cover every measurement the backlog item asks for", () => {
  for (const name of [
    "fps_median", "fps_p5", "frame_ms_p95", "triangles_per_frame", "draw_calls", "backend", "adapter",
    "user_agent", "device_pixel_ratio", "canvas_width", "canvas_height", "lod", "count",
  ]) {
    assert.ok(COLUMN_NAMES.includes(name), `missing column ${name}`);
  }
});

test("results.csv starts with exactly the schema header", () => {
  const text = readFileSync(new URL("results.csv", devices), "utf8");
  assert.equal(text.split(/\r?\n/)[0], csvHeader());
});

test("results.csv rows are complete device rows, never software rendering", () => {
  const rows = parseCsv(readFileSync(new URL("results.csv", devices), "utf8"));
  const [header, ...data] = rows;
  assert.deepEqual(header, COLUMN_NAMES);
  const at = (name) => COLUMN_NAMES.indexOf(name);
  data.forEach((row, i) => {
    const line = i + 2;
    assert.equal(row.length, COLUMN_NAMES.length, `results.csv line ${line}: ${row.length} fields`);
    assert.notEqual(row[at("software")], "yes", `results.csv line ${line}: software-rendered rows are not device results`);
    assert.ok(!row[at("warnings")].startsWith("error:"), `results.csv line ${line}: failed runs do not belong here`);
    assert.ok(Number(row[at("fps_median")]) > 0, `results.csv line ${line}: fps_median must be positive`);
  });
});

test("bench/devices/README.md documents every column, in order", () => {
  const readme = readFileSync(new URL("README.md", devices), "utf8");
  const section = readme.split(/^## CSV columns$/m)[1];
  assert.ok(section, "README needs a '## CSV columns' section");
  const body = section.split(/^## /m)[0];
  const documented = [...body.matchAll(/^\| `([a-z0-9_]+)` \|/gm)].map((m) => m[1]);
  assert.deepEqual(documented, COLUMN_NAMES);
});

test("CSV quoting follows RFC 4180", () => {
  assert.equal(csvEscape("plain"), "plain");
  assert.equal(csvEscape(12.5), "12.5");
  assert.equal(csvEscape(""), "");
  assert.equal(csvEscape(undefined), "");
  assert.equal(csvEscape("a,b"), '"a,b"');
  assert.equal(csvEscape('say "hi"'), '"say ""hi"""');
  assert.equal(csvEscape("two\nlines"), '"two\nlines"');
});

test("a row round-trips through the parser with every column in place", () => {
  const result = Object.fromEntries(COLUMN_NAMES.map((n, i) => [n, i]));
  result.adapter = 'ANGLE (Vendor, "GPU", driver 1,2)';
  result.user_agent = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko)";
  const [header, row] = parseCsv(`${csvHeader()}\n${csvRow(result)}\n`);
  assert.deepEqual(header, COLUMN_NAMES);
  assert.equal(row.length, COLUMN_NAMES.length);
  assert.equal(row[COLUMN_NAMES.indexOf("adapter")], result.adapter);
  assert.equal(row[COLUMN_NAMES.indexOf("user_agent")], result.user_agent);
});

test("missingColumns names absent columns and accepts empty strings", () => {
  const full = Object.fromEntries(COLUMN_NAMES.map((n) => [n, ""]));
  assert.deepEqual(missingColumns(full), []);
  const { fps_median: _drop, ...partial } = full;
  assert.deepEqual(missingColumns(partial), ["fps_median"]);
  assert.deepEqual(missingColumns(null), COLUMN_NAMES);
});
