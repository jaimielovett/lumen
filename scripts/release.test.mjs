import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, relative, isAbsolute } from "node:path";
import { execFileSync, spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("release.mjs", import.meta.url));

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "lumen-release-"));
  t.after(() => {
    const withinTemp = relative(tmpdir(), root);
    assert.ok(withinTemp && !withinTemp.startsWith("..") && !isAbsolute(withinTemp));
    rmSync(root, { recursive: true, force: true });
  });
  mkdirSync(join(root, "src-tauri"));
  return root;
}

test("Windows-only release manifest includes the signed installer and correct fork URL", (t) => {
  const root = fixture(t);
  const installer = join(root, "Lumen_1.1.0_x64-setup.exe");
  writeFileSync(installer, "installer fixture");
  writeFileSync(installer + ".sig", "signature fixture\n");
  execFileSync(process.execPath, [script, "manifest", "1.1.0", "Notes", "x86_64", "jaimielovett/lumen", installer], { cwd: root });
  const output = join(root, "src-tauri/target/release/bundle/release");
  const manifest = JSON.parse(readFileSync(join(output, "latest.json"), "utf8"));
  assert.deepEqual(Object.keys(manifest.platforms), ["windows-x86_64"]);
  assert.equal(manifest.version, "1.1.0");
  assert.equal(manifest.platforms["windows-x86_64"].signature, "signature fixture");
  assert.equal(manifest.platforms["windows-x86_64"].url, "https://github.com/jaimielovett/lumen/releases/download/v1.1.0/Lumen_1.1.0_x64-setup.exe");
  assert.equal(readFileSync(join(output, "Lumen_1.1.0_x64-setup.exe"), "utf8"), "installer fixture");
  assert.equal(readFileSync(join(output, "Lumen_1.1.0_x64-setup.exe.sig"), "utf8"), "signature fixture\n");
});

test("manifest refuses an installer without an updater signature", (t) => {
  const root = fixture(t);
  const installer = join(root, "Lumen_1.1.0_x64-setup.exe");
  writeFileSync(installer, "unsigned installer fixture");
  const result = spawnSync(process.execPath, [script, "manifest", "1.1.0", "Notes", "x86_64", "jaimielovett/lumen", installer], { cwd: root });
  assert.notEqual(result.status, 0);
  assert.match(result.stderr.toString(), /Aucun paquet signé/);
});

test("bump keeps package versions and both lockfiles aligned", (t) => {
  const root = fixture(t);
  writeFileSync(join(root, "package.json"), JSON.stringify({ version: "1.0.1" }));
  writeFileSync(join(root, "package-lock.json"), JSON.stringify({ version: "1.0.1", packages: { "": { version: "1.0.1" } } }));
  writeFileSync(join(root, "src-tauri/tauri.conf.json"), JSON.stringify({ version: "1.0.1" }));
  writeFileSync(join(root, "src-tauri/Cargo.toml"), '[package]\nname = "lumen"\nversion = "1.0.1"\n');
  writeFileSync(join(root, "src-tauri/Cargo.lock"), '[[package]]\nname = "lumen"\nversion = "1.0.1"\n\n[[package]]\nname = "other"\nversion = "2.0.0"\n');
  execFileSync(process.execPath, [script, "bump", "1.1.0"], { cwd: root });
  const lock = JSON.parse(readFileSync(join(root, "package-lock.json"), "utf8"));
  assert.equal(JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version, "1.1.0");
  assert.equal(JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8")).version, "1.1.0");
  assert.equal(lock.version, "1.1.0");
  assert.equal(lock.packages[""].version, "1.1.0");
  assert.match(readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8"), /name = "lumen"\nversion = "1.1.0"/);
  assert.match(readFileSync(join(root, "src-tauri/Cargo.lock"), "utf8"), /name = "lumen"\nversion = "1.1.0"/);
  assert.match(readFileSync(join(root, "src-tauri/Cargo.lock"), "utf8"), /name = "other"\nversion = "2.0.0"/);
});
