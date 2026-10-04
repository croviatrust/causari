/* verify.js — causari.dev/verify: offline verification of an audit seal
 * bundle (causari.audit-seal.v1), a bare crovia.seal.v1 seal, or a
 * seals.jsonl chain. Needs seal-core.js loaded first. Makes no network
 * request; nothing is sent anywhere.
 *
 * The bundle checks mirror src/audit_seal.rs (verify_bundle) step for step,
 * so a bundle that `re seal verify` accepts is accepted here and one it
 * rejects is rejected at the same check. The whole thing is exposed as
 * globalThis.causariVerify so a Node script can drive it without a DOM. */
"use strict";
(function () {
  const BUNDLE_KIND = "causari.audit-seal.v1";
  const INPUT_TYPE = "causari.audit.input.v1";
  const SUBJECT_TYPE = "causari.audit.v1";
  const GENERATOR_ID = "causari";
  const utf8 = new TextEncoder();

  /* ---------------------------------------------------------------- *
   *  Strict JSON: JSON.parse keeps the last of two duplicate keys, so a
   *  bundle with two "audit_json" members would display one text and hash
   *  the other. The Rust side refuses duplicates; so does this parser.
   * ---------------------------------------------------------------- */
  function parseJsonStrict(text) {
    let i = 0;
    const n = text.length;
    const fail = (msg) => { throw new Error("not JSON: " + msg + " at offset " + i); };
    const ws = () => { while (i < n && (text[i] === " " || text[i] === "\t" || text[i] === "\n" || text[i] === "\r")) i++; };
    function value() {
      ws();
      if (i >= n) fail("unexpected end");
      const c = text[i];
      if (c === "{") return object();
      if (c === "[") return array();
      if (c === '"') return string();
      if (c === "t") return literal("true", true);
      if (c === "f") return literal("false", false);
      if (c === "n") return literal("null", null);
      if (c === "-" || (c >= "0" && c <= "9")) return number();
      fail("unexpected character " + JSON.stringify(c));
    }
    function literal(word, v) {
      if (text.substr(i, word.length) !== word) fail("bad literal");
      i += word.length;
      return v;
    }
    function number() {
      const m = /^-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?/.exec(text.slice(i, i + 64));
      if (!m) fail("bad number");
      i += m[0].length;
      return Number(m[0]);
    }
    function string() {
      i++; // opening quote
      let out = "";
      for (;;) {
        if (i >= n) fail("unterminated string");
        const c = text[i++];
        if (c === '"') return out;
        if (c === "\\") {
          const e = text[i++];
          if (e === '"' || e === "\\" || e === "/") out += e;
          else if (e === "b") out += "\b";
          else if (e === "f") out += "\f";
          else if (e === "n") out += "\n";
          else if (e === "r") out += "\r";
          else if (e === "t") out += "\t";
          else if (e === "u") {
            const h = text.substr(i, 4);
            if (!/^[0-9a-fA-F]{4}$/.test(h)) fail("bad \\u escape");
            out += String.fromCharCode(parseInt(h, 16));
            i += 4;
          } else fail("bad escape");
        } else if (c < " ") fail("control character in string");
        else out += c;
      }
    }
    function array() {
      i++;
      const out = [];
      ws();
      if (text[i] === "]") { i++; return out; }
      for (;;) {
        out.push(value());
        ws();
        if (text[i] === ",") { i++; continue; }
        if (text[i] === "]") { i++; return out; }
        fail("expected , or ]");
      }
    }
    function object() {
      i++;
      const out = {};
      ws();
      if (text[i] === "}") { i++; return out; }
      for (;;) {
        ws();
        if (text[i] !== '"') fail("expected string key");
        const k = string();
        if (Object.prototype.hasOwnProperty.call(out, k)) fail("duplicate key " + JSON.stringify(k));
        ws();
        if (text[i] !== ":") fail("expected :");
        i++;
        out[k] = value();
        ws();
        if (text[i] === ",") { i++; continue; }
        if (text[i] === "}") { i++; return out; }
        fail("expected , or }");
      }
    }
    const v = value();
    ws();
    if (i < n) fail("trailing characters");
    return v;
  }

  /* ---------------------------------------------------------------- *
   *  Audit seal bundle (mirrors audit_seal::verify_bundle)
   * ---------------------------------------------------------------- */
  const isObject = (v) => v !== null && typeof v === "object" && !Array.isArray(v);
  const isBundle = (v) => isObject(v) && v.bundle === BUNDLE_KIND;

  function get(v, path) {
    let cur = v;
    for (const k of path) {
      if (!isObject(cur) || !(k in cur)) throw new Error("bundle: missing " + path.join("."));
      cur = cur[k];
    }
    return cur;
  }
  function getStr(v, path) {
    const x = get(v, path);
    if (typeof x !== "string") throw new Error("bundle: " + path.join(".") + " must be a string");
    return x;
  }
  function onlyKeys(obj, allowed, what) {
    const extra = Object.keys(obj).filter((k) => !allowed.includes(k));
    if (extra.length) throw new Error("bundle: unknown field " + (what ? what + "." : "") + extra[0] + " (fail-closed)");
  }

  async function verifyBundle(bundle, steps) {
    const step = (ok, label, det) => { steps.push({ ok, label, det: det || "" }); if (!ok) throw new Error(label + (det ? ": " + det : "")); };
    const check = (label, fn) => { let det = ""; try { det = fn() || ""; } catch (e) { step(false, label, e.message); } step(true, label, det); };

    if (!isObject(bundle)) step(false, "bundle is a JSON object");
    check("bundle has only bundle, seal, subject", () => onlyKeys(bundle, ["bundle", "seal", "subject"], ""));
    step(isBundle(bundle), "bundle is " + BUNDLE_KIND, String(bundle.bundle));
    const sealed = get(bundle, ["seal"]);
    const subject = get(bundle, ["subject"]);
    check("subject has only input, audit_json", () => { if (!isObject(subject)) throw new Error("subject must be an object"); onlyKeys(subject, ["input", "audit_json"], "subject"); });

    // 1. The seal on its own terms.
    steps.push({ ok: true, label: "— the seal (crovia.seal.v1)", det: sealed && sealed.seal_id || "", hdr: true });
    const phash = await verifySeal(sealed, steps);

    // 2. Output: the audit JSON bytes.
    steps.push({ ok: true, label: "— subject.output: the audit JSON", det: "", hdr: true });
    const auditText = getStr(bundle, ["subject", "audit_json"]);
    const auditBytes = utf8.encode(auditText);
    const wantOut = getStr(sealed, ["subject", "output_hash"]);
    const gotOut = "sha256:" + (await sha256hex(auditBytes));
    step(wantOut === gotOut, "audit JSON hash matches subject.output_hash", wantOut === gotOut ? gotOut.slice(0, 30) + "…" : "the numbers were altered");
    step(get(sealed, ["subject", "output_len"]) === auditBytes.length, "audit JSON length matches subject.output_len", auditBytes.length + " bytes");
    let audit;
    check("audit JSON parses (strict) to an object", () => { audit = parseJsonStrict(auditText); if (!isObject(audit)) throw new Error("audit JSON is not an object"); });

    // 3. Input: the audited git state, CSC-1 canonical.
    steps.push({ ok: true, label: "— subject.input: the audited git state", det: "", hdr: true });
    const input = get(bundle, ["subject", "input"]);
    check("subject.input has only type, commit, method, options", () => { if (!isObject(input)) throw new Error("subject.input must be an object"); onlyKeys(input, ["type", "commit", "method", "options"], "subject.input"); });
    step(getStr(input, ["type"]) === INPUT_TYPE, "subject.input.type is " + INPUT_TYPE, String(input.type));
    let inputBytes;
    check("subject.input is CSC-1 canonical", () => { inputBytes = utf8.encode(csc1(input)); return inputBytes.length + " bytes"; });
    const wantIn = getStr(sealed, ["subject", "input_hash"]);
    const gotIn = "sha256:" + (await sha256hex(inputBytes));
    step(wantIn === gotIn, "input hash matches subject.input_hash", wantIn === gotIn ? gotIn.slice(0, 30) + "…" : "the commit or options were altered");
    step(get(sealed, ["subject", "input_len"]) === inputBytes.length, "input length matches subject.input_len", inputBytes.length + " bytes");
    step(getStr(sealed, ["subject", "modality"]) === "text", "subject.modality is text", String(sealed.subject.modality));

    // 4. Binding: input, generator params and the audit agree.
    steps.push({ ok: true, label: "— binding: input, signed params, audit JSON", det: "", hdr: true });
    step(getStr(sealed, ["generator", "id"]) === GENERATOR_ID, "generator.id is " + GENERATOR_ID, String(sealed.generator.id));
    const params = get(sealed, ["generator", "params"]);
    step(getStr(params, ["subject_type"]) === SUBJECT_TYPE, "generator.params.subject_type is " + SUBJECT_TYPE, String(params.subject_type));
    const commit = getStr(input, ["commit"]);
    step(/^[0-9a-fA-F]{40}$/.test(commit), "subject.input.commit is a 40-hex commit hash", commit);
    step(getStr(params, ["commit"]) === commit, "generator.params.commit equals subject.input.commit", "");
    const method = getStr(input, ["method"]);
    step(getStr(params, ["method"]) === method, "generator.params.method equals subject.input.method", method);
    step(getStr(audit, ["method"]) === method, "audit JSON method equals the sealed method", "");
    step(getStr(audit, ["coverage", "method"]) === method, "audit JSON coverage.method equals the sealed method", "");
    const shallow = get(audit, ["coverage", "shallow"]);
    step(typeof shallow === "boolean", "audit JSON coverage.shallow is a boolean", String(shallow));
    step(getStr(params, ["coverage.shallow"]) === String(shallow), "generator.params.coverage.shallow equals the audit JSON", String(shallow));
    const allowShallow = get(input, ["options", "allow_shallow"]);
    step(typeof allowShallow === "boolean", "subject.input.options.allow_shallow is a boolean", String(allowShallow));
    step(!(shallow && !allowShallow), "a shallow audit was allowed explicitly", shallow ? "--allow-shallow" : "full history");

    return {
      seal_id: getStr(sealed, ["seal_id"]),
      issuer_id: getStr(sealed, ["issuer", "id"]),
      pubkey_hex: getStr(sealed, ["issuer", "pubkey", "key_hex"]),
      sequence: get(sealed, ["chain", "sequence"]),
      prev_seal_hash: get(sealed, ["chain", "prev_seal_hash"]),
      payload_hash: "sha256:" + phash,
      emitted_at: getStr(sealed, ["timestamp", "emitted_at"]),
      generator_version: typeof sealed.generator.version === "string" ? sealed.generator.version : null,
      commit,
      method,
      repo: getStr(params, ["repo"]),
      shallow,
      audit,
    };
  }

  /* ---------------------------------------------------------------- *
   *  Any document: bundle, bare seal, or JSONL chain
   * ---------------------------------------------------------------- */
  function parseDocument(text) {
    const trimmed = text.trim();
    if (!trimmed) throw new Error("empty input");
    try {
      return { value: parseJsonStrict(trimmed), chain: false };
    } catch (whole) {
      // seals.jsonl: one seal per line. A pretty-printed document starts
      // with a lone bracket and is reported with its own error.
      const lines = trimmed.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);
      if (lines.length < 2 || lines[0] === "{" || lines[0] === "[") throw whole;
      const seals = [];
      for (let k = 0; k < lines.length; k++) {
        try { seals.push(parseJsonStrict(lines[k])); } catch (e) { throw new Error("line " + (k + 1) + ": " + e.message); }
      }
      return { value: seals, chain: true };
    }
  }

  /* Returns { valid, kind: "audit"|"seal"|"chain"|"unreadable", reason?, statement? }.
   * Never throws. `steps` collects {ok,label,det,hdr?} in verification order. */
  async function verifyDocument(text, steps) {
    steps = steps || [];
    let doc;
    try { doc = parseDocument(text); } catch (e) { return { valid: false, kind: "unreadable", reason: e.message, steps }; }
    const v = doc.value;
    try {
      if (doc.chain || Array.isArray(v)) {
        const seals = v;
        if (!seals.length) return { valid: false, kind: "unreadable", reason: "empty chain", steps };
        await verifyChain(seals, steps);
        const last = seals[seals.length - 1];
        return { valid: true, kind: "chain", steps, statement: { seals: seals.length, issuer_id: last.issuer && last.issuer.id, pubkey_hex: last.issuer && last.issuer.pubkey && last.issuer.pubkey.key_hex, first_sequence: seals[0].chain.sequence, last_sequence: last.chain.sequence, last_seal_id: last.seal_id } };
      }
      if (isBundle(v)) {
        const statement = await verifyBundle(v, steps);
        return { valid: true, kind: "audit", steps, statement };
      }
      if (!isObject(v)) return { valid: false, kind: "unreadable", reason: "not a seal: top-level value is not an object", steps };
      const phash = await verifySeal(v, steps);
      return { valid: true, kind: "seal", steps, statement: { seal_id: v.seal_id, issuer_id: v.issuer.id, pubkey_hex: v.issuer.pubkey.key_hex, sequence: v.chain.sequence, prev_seal_hash: v.chain.prev_seal_hash, payload_hash: "sha256:" + phash, emitted_at: v.timestamp && v.timestamp.emitted_at, generator: v.generator, subject: v.subject } };
    } catch (e) {
      const kind = doc.chain || Array.isArray(v) ? "chain" : isBundle(v) ? "audit" : "seal";
      return { valid: false, kind, reason: e.message, steps };
    }
  }

  globalThis.causariVerify = { parseJsonStrict, verifyBundle, verifyDocument, isBundle, BUNDLE_KIND, INPUT_TYPE, SUBJECT_TYPE, GENERATOR_ID };

  /* ---------------------------------------------------------------- *
   *  Page wiring (absent under Node)
   * ---------------------------------------------------------------- */
  if (typeof document === "undefined") return;

  const $ = (id) => document.getElementById(id);
  const text = $("vf-text"), run = $("vf-run"), file = $("vf-file"), clear = $("vf-clear"), drop = $("vf-drop"), fname = $("vf-filename");
  const verdict = $("vf-verdict"), states = $("vf-states"), auditTbl = $("vf-audit"), means = $("vf-means"), stepsEl = $("vf-steps");
  // The result panel explains what will appear until something does.
  const result = $("vf-result");
  const showResult = (on) => { if (result) result.classList.toggle("has-result", on); };
  const pct = (x) => (typeof x === "number" ? (x * 100).toFixed(1) + " %" : "n/a");

  ed25519Supported().then((ok) => { if (!ok) $("vf-unsupported").hidden = false; });

  function el(tag, cls, txt) { const e = document.createElement(tag); if (cls) e.className = cls; if (txt !== undefined) e.textContent = txt; return e; }
  function row(k, v, mono) {
    states.appendChild(el("dt", "", k));
    const dd = el("dd");
    if (mono) dd.appendChild(el("code", "", v)); else dd.textContent = v;
    states.appendChild(dd);
  }
  function reset() {
    verdict.textContent = "—"; verdict.removeAttribute("data-valid");
    states.replaceChildren(); stepsEl.replaceChildren(); means.textContent = "";
    auditTbl.hidden = true; auditTbl.tBodies[0].replaceChildren();
    showResult(false);
  }
  function renderSteps(steps) {
    for (const s of steps) {
      const li = el("li", s.hdr ? "hdr" : s.ok ? "ok" : "fail");
      li.appendChild(el("span", "", s.hdr ? "" : s.ok ? "✓" : "✗"));
      const body = el("span", "", s.label);
      if (s.det) body.appendChild(el("span", "det", s.det));
      li.appendChild(body);
      stepsEl.appendChild(li);
    }
  }
  function renderAudit(a) {
    const tb = auditTbl.tBodies[0];
    for (const [label, s] of [["AI-tagged (metadata matched)", a.verified], ["probable (heuristic)", a.probable]]) {
      if (!isObject(s)) continue;
      const tr = el("tr");
      for (const cell of [label, s.commits, s.introduced, s.surviving, pct(s.survival_rate), pct(s.capped_survival_rate), pct(s.median_survival)]) tr.appendChild(el("td", "", String(cell)));
      tb.appendChild(tr);
    }
    auditTbl.hidden = false;
  }
  async function verifyNow() {
    reset();
    showResult(true);
    verdict.textContent = "verifying…";
    const steps = [];
    const r = await verifyDocument(text.value, steps);
    verdict.dataset.valid = String(r.valid);
    if (!r.valid) {
      verdict.textContent = (r.kind === "unreadable" ? "✗ unreadable: " : "✗ invalid: ") + r.reason;
      renderSteps(steps);
      return;
    }
    const s = r.statement;
    if (r.kind === "audit") {
      verdict.textContent = "✓ signature valid — audit seal " + s.seal_id;
      row("issuer", s.issuer_id, true);
      row("pubkey", s.pubkey_hex, true);
      row("chain", "sequence " + s.sequence + (s.prev_seal_hash ? ", follows " + s.prev_seal_hash.slice(0, 23) + "…" : " (genesis)"));
      row("emitted", s.emitted_at);
      row("repo", s.repo, true);
      row("commit", s.commit, true);
      row("method", s.method + (s.shallow ? " · shallow clone: history truncated, figures partial" : ""));
      if (s.generator_version) row("causari", s.generator_version);
      row("audit", (s.audit.total_commits ?? "?") + " commits analysed");
      renderAudit(s.audit);
      means.textContent = "Means: the issuer key signed this exact audit JSON for this commit with this method; the numbers were not altered since. It does not prove they are true — rerun `re audit` on the commit to check.";
    } else if (r.kind === "seal") {
      verdict.textContent = "✓ signature valid — seal " + s.seal_id;
      row("issuer", s.issuer_id, true);
      row("pubkey", s.pubkey_hex, true);
      row("chain", "sequence " + s.sequence + (s.prev_seal_hash ? ", follows " + s.prev_seal_hash.slice(0, 23) + "…" : " (genesis)"));
      row("emitted", s.emitted_at || "?");
      row("generator", (s.generator && s.generator.id) + (s.generator && s.generator.version ? " " + s.generator.version : ""));
      row("subject", (s.subject && s.subject.modality) + " · input " + (s.subject && s.subject.input_len) + " B · output " + (s.subject && s.subject.output_len) + " B");
      means.textContent = "Means: this issuer key signed a receipt committing to the hashes of one input and one output. Without the bytes themselves, only the receipt is checked, not what it covers.";
    } else {
      verdict.textContent = "✓ " + s.seals + " seal(s) verified — every signature valid, chain contiguous";
      row("issuer", s.issuer_id, true);
      row("pubkey", s.pubkey_hex, true);
      row("range", "sequence " + s.first_sequence + " → " + s.last_sequence);
      row("last", s.last_seal_id, true);
    }
    renderSteps(steps);
  }

  function load(f) {
    if (!f) return;
    fname.textContent = f.name + " · " + f.size + " bytes";
    f.text().then((t) => { text.value = t; verifyNow(); });
  }
  // The embedded sample: a real seal, then the same seal with one digit of
  // the audit JSON changed, so both outcomes are one click away.
  const sampleEl = document.getElementById("vf-sample-data");
  const sampleBtn = document.getElementById("vf-sample");
  const tamperBtn = document.getElementById("vf-tamper");
  function sampleText() { return sampleEl ? sampleEl.textContent.trim() : ""; }
  if (sampleBtn) sampleBtn.addEventListener("click", () => {
    const t = sampleText(); if (!t) return;
    text.value = JSON.stringify(JSON.parse(t), null, 2); fname.textContent = "sample: this repository's audit seal"; file.value = ""; verifyNow();
  });
  if (tamperBtn) tamperBtn.addEventListener("click", () => {
    const t = sampleText(); if (!t) return;
    const b = JSON.parse(t);
    // Raise the surviving-lines count of the first agent by one, inside the sealed audit JSON.
    b.subject.audit_json = b.subject.audit_json.replace(/("surviving":\s*)(\d+)/, (m, k, n) => k + String(Number(n) + 1));
    text.value = JSON.stringify(b, null, 2); fname.textContent = "sample with one digit changed"; file.value = ""; verifyNow();
  });
  run.addEventListener("click", verifyNow);
  clear.addEventListener("click", () => { text.value = ""; fname.textContent = ""; file.value = ""; reset(); });
  file.addEventListener("change", () => load(file.files[0]));
  text.addEventListener("keydown", (e) => { if ((e.ctrlKey || e.metaKey) && e.key === "Enter") verifyNow(); });
  for (const ev of ["dragenter", "dragover"]) drop.addEventListener(ev, (e) => { e.preventDefault(); drop.classList.add("is-over"); });
  for (const ev of ["dragleave", "drop"]) drop.addEventListener(ev, (e) => { e.preventDefault(); drop.classList.remove("is-over"); });
  drop.addEventListener("drop", (e) => load(e.dataTransfer.files[0]));
})();
