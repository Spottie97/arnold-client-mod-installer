#!/usr/bin/env node
"use strict";

const fs = require("fs");

const path = process.argv[2] || "src-tauri/tauri.updater.conf.json";
const key = (process.env.TAURI_UPDATER_PUBKEY || "").trim();
if (!key) {
  console.error("TAURI_UPDATER_PUBKEY is empty");
  process.exit(1);
}

const conf = JSON.parse(fs.readFileSync(path, "utf8"));
conf.plugins = conf.plugins || {};
conf.plugins.updater = conf.plugins.updater || {};
conf.plugins.updater.pubkey = key;
fs.writeFileSync(path, JSON.stringify(conf, null, 2) + "\n");

const written = JSON.parse(fs.readFileSync(path, "utf8"));
const pubkey = (written.plugins && written.plugins.updater && written.plugins.updater.pubkey) || "";
if (!pubkey) {
  console.error("plugins.updater.pubkey was not written");
  process.exit(1);
}
console.log(
  "Wrote plugins.updater.pubkey into " + path + " (length " + pubkey.length + ", value not printed)."
);
