"use strict";

// Run with node --test uac2-control/tests/ui.test.cjs.
// Use a locally installed Playwright, or set UAC2_PLAYWRIGHT_MODULE to its path.
// UAC2_BROWSER_PATH optionally selects an installed Chromium-compatible browser.
const { before, after, beforeEach, afterEach, describe, it } = require("node:test");
const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const path = require("node:path");
const { chromium } = require(process.env.UAC2_PLAYWRIGHT_MODULE || "playwright");

const project = path.resolve(__dirname, "..");
const read = file => readFileSync(path.join(project, file), "utf8");
const icon = Buffer.from(read("assets/uac2-icon.svg")).toString("base64");
const html = read("ui/index.html")
  .replace("__ICON__", "data:image/svg+xml;base64," + icon)
  .replace("<!--STYLE-->", () => "<style>" + read("ui/style.css") + "</style>")
  .replace("<!--SCRIPT-->", () => "<script>" + read("ui/main.js") + "</script>");

const headphones = {
  id: "headphones", name: "Fixture headphones", supported: true, reason: "",
  function_id: "Fixture USB Audio", physical_id: "test usb", container: "test",
  arrival: "11", siblings: ["Fixture headphones", "Fixture microphone"],
  rate: 44100, bits: 24, container_bits: 32, channels: 2, encoding: "PCM", raw_format: "24b",
  mix_rate: 44100, mix_bits: 32, mix_encoding: "Float", mix_channels: 2,
  min: 309, default: 441, fundamental: 1, max: 441, current: 441, current_rate: 44100,
  snapshot: false,
};
const speakers = {
  ...headphones, id: "speakers", name: "Fixture speakers", function_id: "Second fixture",
  rate: 48000, mix_rate: 48000, min: 144, default: 480, max: 480, current: 240,
  current_rate: 48000, snapshot: true, siblings: ["Fixture speakers"],
};
const unsupported = {
  ...speakers, id: "other", name: "Other device", supported: false, reason: "Unsupported driver", current: 0,
};
const endpoints = [headphones, speakers, unsupported];
const nativePath = "\\\\?\\X:\\Fixture\\current";
const displayPath = "X:\\Fixture\\current";
const emptyDirectory = {type: "directory", checked: true, ok: false, path: null, message: ""};
const validDirectory = {type: "directory", ok: true, path: nativePath};
const validated = {...validDirectory, checked: true};

describe("Packaged UAC2 interface with a simulated native host", {concurrency: false}, () => {
  let browser, page, errors;
  const node = id => page.locator("#" + id);
  const visible = id => node(id).isVisible();
  const content = id => node(id).innerText();
  const click = id => node(id).click();
  const button = name => page.getByRole("button", {name, exact: true});
  const dialogButton = name => page.getByRole("dialog").getByRole("button", {name, exact: true});
  const emit = message => page.evaluate(message => window.onNative(message), message);
  const messages = op => page.evaluate(op => window.testMessages.filter(m => !op || m.op === op), op);
  const sends = async op => (await messages(op)).length;
  const latest = async () => (await messages()).at(-1);
  const devices = (items = endpoints, selected = headphones.id, automatic = false) =>
    emit({type: "devices", devices: items, selected, automatic});

  before(async () => {
    browser = await chromium.launch({
      headless: true,
      executablePath: process.env.UAC2_BROWSER_PATH || undefined,
      args: ["--disable-background-networking", "--mute-audio"],
    });
  });
  after(async () => { if (browser) await browser.close(); });
  beforeEach(async () => {
    page = await browser.newPage({viewport: {width: 1120, height: 780}});
    page.setDefaultTimeout(4000);
    errors = [];
    page.on("pageerror", e => errors.push(e.message));
    await page.evaluate(() => {
      window.testMessages = [];
      window.ipc = {postMessage: value => window.testMessages.push(JSON.parse(value))};
    });
    await page.setContent(html);
    assert.equal((await latest()).op, "ready");
  });
  afterEach(async () => {
    if (page) await page.close();
    assert.deepEqual(errors, []);
  });

  async function open(selected = headphones.id, prefs = {}, items = endpoints) {
    await emit({type: "init", accepted: true, theme: "dark", lang: "zh", hasDir: false, ...prefs});
    assert.equal((await latest()).op, "list");
    await devices(items, selected);
  }
  async function prepare(response = validated) {
    await click("bench");
    assert.equal((await latest()).op, "checkDir");
    await emit(response);
  }
  async function checkCompactPreparation() {
    const panel = node("dialog-content");
    await panel.evaluate(element => { element.scrollTop = 0; });
    await panel.hover();
    await page.mouse.wheel(0, 800);
    await page.waitForFunction(() => ["choose-dir", "start-test"].every(id => {
      const control = document.getElementById(id);
      const rect = control.getBoundingClientRect();
      const hit = document.elementFromPoint(rect.x + rect.width / 2, rect.y + rect.height / 2);
      return rect.top >= 0 && rect.bottom <= innerHeight && control.contains(hit);
    }));
  }
  async function start() {
    await prepare();
    await dialogButton("开始测试").click();
    assert.equal((await latest()).op, "bench");
  }

  it("keeps window controls usable on the main page and during a device operation", async () => {
    await open(speakers.id,{lang:"en"});
    const heading=await page.locator("h1").boundingBox();
    const wideProbe=await node("diagnostics").boundingBox();
    const about=await node("about").boundingBox();
    for(const item of [wideProbe,about])
      assert.ok(item.y<heading.y+heading.height && item.y+item.height>heading.y);
    await page.setViewportSize({width:600,height:520});
    const probe=await node("diagnostics").boundingBox();
    const minimize=button("Minimize");
    const first=await minimize.boundingBox();
    assert.ok(probe.x+probe.width<=first.x && first.y===0);
    await minimize.click();
    assert.deepEqual(await latest(),{op:"windowMinimize"});
    await button("Maximize").click();
    assert.deepEqual(await latest(),{op:"windowMaximize"});
    await emit({type:"windowState",maximized:true});
    await button("Restore window").click();
    assert.deepEqual(await latest(),{op:"windowMaximize"});
    await click("apply");
    await dialogButton("Apply settings").click();
    await dialogButton("Close window").click();
    assert.deepEqual(await latest(),{op:"exit"});
  });

  it("generates reports only on demand for the selected output and opens the fixed issue action separately",async()=>{
    const path="X:\\Fixture\\uac2-data\\diagnostics\\latest.json";
    await open(unsupported.id,{diagnosticPath:path});
    await click("diagnostics");
    assert.match(await content("modal"),/声卡品牌、型号/);
    assert.match(await content("modal"),/Other device/);
    assert.equal(await node("diagnostic-target-"+unsupported.id).innerText(),unsupported.name);
    assert.equal(await node("endpoint-"+unsupported.id).getAttribute("value"),unsupported.id);
    assert.equal(await page.locator(".report-path").innerText(),path);
    assert.equal(await sends("generateDiagnostic"),0);
    await click("generate-report");
    const request=await latest();
    assert.deepEqual(request,{op:"generateDiagnostic",id:unsupported.id,requestId:request.requestId});
    assert.equal(await node("generate-report").isDisabled(),true);
    assert.equal(await sends("checkDir"),0);
    await emit({...request,type:"diagnostic",id:speakers.id,path});
    assert.match(await content("modal"),/正在读取/);
    await emit({...request,type:"error",message:"Fixture read failed"});
    assert.match(await content("modal"),/生成失败.*Fixture read failed/);
    await click("generate-report");
    await emit({...await latest(),type:"diagnostic",path});
    assert.match(await content("modal"),/报告已保存/);
    assert.equal(await sends("openIssue"),0);
    await click("issue-link");
    assert.deepEqual(await latest(),{op:"openIssue"});
    await emit({type:"error",op:"openIssue",message:"Explorer unavailable"});
    assert.match(await content("modal"),/Explorer unavailable/);
    await devices([],"",true);
    assert.equal(await node("generate-report").isDisabled(),true);
    assert.match(await content("modal"),/选择一个活动播放输出/);
    await page.keyboard.press("Escape");
    assert.equal(await node("diagnostics").evaluate(n=>n===document.activeElement),true);
  });

  it("presents the product, version and license and opens the named GitHub icon by keyboard",async()=>{
    await open(speakers.id,{lang:"en",version:"0.2.1"});
    for(const [lang,theme,width,purpose,license,label] of [
      ["en","light",600,/silent playback test/,"License · PolyForm Noncommercial 1.0.0","Source on GitHub"],
      ["zh","dark",360,/静音试流/,"许可 · PolyForm Noncommercial 1.0.0","GitHub 源码"]
    ]) {
      await page.setViewportSize({width,height:640});
      await click(lang);
      await click(theme);
      await click("about");
      assert.equal(await node("dialog-title").textContent(),"UAC2 Control 0.2.1");
      assert.match(await content("modal"),purpose);
      const footer=page.locator("#dialog-content p").last();
      assert.equal(await footer.innerText(),license);
      const licenseBox=await footer.boundingBox();
      assert.ok(licenseBox.x>=0 && licenseBox.x+licenseBox.width<=width);
      assert.equal(await node("repository-link").getAttribute("aria-label"),label);
      assert.equal(await node("repository-link").getAttribute("title"),label);
      assert.equal(await node("repository-link").locator("svg").count(),1);
      assert.equal(await node("safe-action").evaluate(n=>n===document.activeElement),true);
      await page.keyboard.press("Tab");
      assert.equal(await node("repository-link").evaluate(n=>n===document.activeElement),true);
      await page.keyboard.press("Enter");
      assert.deepEqual(await latest(),{op:"openRepository"});
      await page.keyboard.press("Escape");
      assert.equal(await node("about").evaluate(n=>n===document.activeElement),true);
    }
  });

  it("balances the workspace in a maximized window and preserves scrolling when more content appears",async()=>{
    await open(speakers.id,{lang:"en",theme:"light"});
    for(const [width,height] of [[1280,1024],[1536,864],[1920,1080],[2560,1440],[1080,1920],[864,1536]]){
      await page.setViewportSize({width,height});
      const [bar,device,controls,results,footer]=await Promise.all(
        [".brand",".device-card",".controls",".results",".footer"].map(s=>page.locator(s).boundingBox())
      );
      if(height>width){
        assert.equal(controls.x,results.x);
        assert.ok(results.y>=controls.y+controls.height);
      } else assert.equal(controls.y,results.y);
      assert.ok(device.width>=width*.70 && device.x>0);
      const topGap=device.y-(bar.y+bar.height),bottomGap=height-(footer.y+footer.height);
      assert.ok(topGap>=0 && bottomGap>=0 && Math.abs(topGap-bottomGap)<=12);
      assert.ok(footer.y+footer.height-device.y>=height*.65);
    }
    await page.setViewportSize({width:1200,height:800});
    await emit({type:"error",op:"prefs",message:Array(14).fill("Fixture storage warning").join("\n")});
    await node("restore").scrollIntoViewIfNeeded();
    assert.equal(await node("restore").isEnabled(),true);
    assert.ok(await page.evaluate(()=>window.scrollY>0 && document.documentElement.scrollWidth<=innerWidth));
    await click("restore");
    assert.equal(await visible("modal"),true);
    await page.keyboard.press("Escape");
  });

  it("requires persisted consent and exits on No or Escape", async () => {
    await emit({type: "init", accepted: false, theme: "dark", lang: "zh", hasDir: false});
    assert.equal(await visible("workspace"), false);
    await dialogButton("否，退出").click();
    assert.equal((await latest()).op, "exit");
    await page.keyboard.press("Escape");
    assert.equal(await sends("exit"), 2);
    await dialogButton("English").click();
    assert.equal((await latest()).lang, "en");
    await dialogButton("Yes, continue").click();
    assert.equal((await latest()).op, "accept");
    assert.equal(await visible("workspace"), false);
    await open();
    assert.equal(await visible("workspace"), true);
  });

  it("preserves read-only tools when snapshots cannot be imported and explains preference failures",async()=>{
    await emit({type:"init",accepted:false,lang:"zh",preferencesError:"Fixture unreadable"});
    assert.match(await content("modal"),/未能读取原有同意记录或偏好设置.*Fixture unreadable/);
    await dialogButton("是，继续使用").click();
    await emit({type:"error",op:"accept",message:"无法保存同意记录"});
    assert.match(await content("modal"),/无法保存同意记录/);
    await dialogButton("仅本次同意并继续").click();
    assert.deepEqual(await latest(),{op:"acceptSession"});
    await open(speakers.id,{lang:"en",sessionConsent:true,migrationError:"Fixture migration error"});
    assert.match(await content("storage-status"),/Recovery snapshots.*Fixture migration error/);
    assert.match(await content("storage-status"),/this session/);
    for(const id of ["apply","hard-restart"]) assert.equal(await node(id).isDisabled(),true);
    for(const id of ["restore","force","bench","diagnostics","refresh"]) assert.equal(await node(id).isEnabled(),true);
    await click("restore");
    assert.match(await content("modal"),/Recovery snapshots could not be loaded/);
    assert.equal(await sends("force"),0);
    await dialogButton("Cancel").click();
  });

  it("refreshes the three real Windows readings and identifies stale or missing values", async () => {
    await open();
    assert.equal(await node("read-list").locator("dt").allTextContents().then(a => a.join("/")), "已公布下限/默认周期/当前周期");
    assert.match(await content("read-list"), /7\.007 ms/);
    assert.match(await content("read-list"), /441 帧/);
    assert.equal(await content("refresh"), "");
    assert.equal(await node("refresh").getAttribute("aria-label"), "刷新 Windows 实读");
    assert.equal(await node("refresh").getAttribute("title"), "刷新 Windows 实读");
    await click("refresh");
    assert.deepEqual(await latest(), {op: "refresh", id: headphones.id});
    await emit({type: "error", op: "refresh", message: "Read temporarily failed"});
    assert.match(await content("stamp"), /上次读取/);
    assert.equal(await node("apply").isDisabled(), true);
    await click("refresh");
    await devices();
    assert.equal(await node("apply").isEnabled(), true);
    await node("output").selectOption(unsupported.id);
    assert.equal((await latest()).id, unsupported.id);
    await devices(endpoints, unsupported.id);
    assert.match(await content("read-list"), /未读到/);
    assert.equal(await node("force").isDisabled(), true);
  });

  it("uses an aligned frame count and warns strictly below 3 ms", async () => {
    await open();
    await node("slider").fill("3");
    assert.equal(await visible("low"), false);
    assert.match(await content("frames"), /133/);
    assert.match(await content("rounded"), /3\.016 ms/);
    await node("slider").fill("2.9");
    assert.equal(await visible("low"), true);
    assert.match(await content("read-list"), /7\.007 ms/);
    await click("all");
    await click("apply");
    assert.match(await content("modal"), /全部采样率/);
    await dialogButton("应用设置").click();
    const write = await latest();
    assert.equal(write.op, "apply");
    assert.equal(write.ms, 2.9);
    assert.equal(write.all, true);
    assert.equal(write.device.id, headphones.id);
    await dialogButton("返回主界面").click();
    assert.equal(await node("apply").isDisabled(), true);
    await emit({type: "operation", ok: true, message: "Fixture confirmed"});
    assert.equal((await latest()).op, "refresh");
  });

  it("guards snapshot restore and shows hardware-specific force-restore scope", async () => {
    await open();
    assert.equal(await node("restore").isEnabled(), true);
    assert.match(await content("restore-help"), /没有匹配/);
    await click("restore");
    assert.match(await content("modal"), /没有匹配/);
    assert.match(await content("modal"), /Fixture microphone/);
    assert.match(await content("modal"), /Fixture USB Audio/);
    assert.match(await content("modal"), /USB 端口/);
    assert.match(await content("modal"), /退出.*应用/);
    assert.match(await content("modal"), /快照/);
    assert.equal(await dialogButton("取消").evaluate(n => n === document.activeElement), true);
    await devices([{...headphones, arrival: "12"}, speakers], headphones.id, true);
    assert.equal(await visible("modal"), false);
    assert.equal(await sends("force"), 0);
    await node("output").selectOption(speakers.id);
    await devices(endpoints, speakers.id);
    await click("restore");
    assert.match(await content("modal"), /首次修改/);
    await dialogButton("取消").click();
    assert.equal(await sends("restore"), 0);
  });

  it("allows confirmed Windows rebuild without period records and rejects an unverified replacement",async()=>{
    const damaged={...speakers,can_rebuild:true,supported:false,min:0,default:0,fundamental:0,max:0};
    await open(damaged.id,{lang:"en"},[damaged]);
    assert.equal(await node("apply").isDisabled(),true);
    await click("restore");
    assert.match(await content("modal"),/period capability records.*could not be verified/);
    assert.equal(await sends("force"),0);
    await devices([{...damaged,can_rebuild:false}],damaged.id,true);
    assert.equal(await visible("modal"),false);
    assert.equal(await node("restore").isDisabled(),true);
    await devices([damaged],damaged.id,true);
    await click("force");
    assert.match(await content("modal"),/quit.*apps/i);
    assert.match(await content("modal"),/snapshot/i);
    await dialogButton("Confirm force restore").click();
    assert.equal((await latest()).op,"force");
    assert.equal((await latest()).device.id,damaged.id);
  });

  async function forceRestore() {
    await click("force");
    await dialogButton("Confirm force restore").click();
    const request=await latest();
    assert.equal(request.op,"force");
    return {type:"operation",op:"force",id:request.device.id,requestId:request.requestId};
  }

  it("refreshes the verified rebuilt output and restores its new snapshot without another removal",async()=>{
    const returned={...headphones,id:"new-headphones",snapshot:true};
    await open(headphones.id,{lang:"en"});
    const request=await forceRestore();
    await emit({...request,requestId:request.requestId+1,ok:true,returnedId:speakers.id});
    assert.equal((await latest()).op,"force");
    await emit({...request,ok:true,returnedId:returned.id,snapshotStatus:"saved",message:"Snapshot saved"});
    assert.deepEqual(await latest(),{op:"refresh",id:returned.id});
    await devices([speakers,returned],returned.id);
    assert.equal(await node("output").inputValue(),returned.id);
    await click("restore");
    await dialogButton("Restore factory defaults").click();
    assert.equal((await latest()).op,"restore");
    assert.equal((await latest()).device.id,returned.id);
    assert.equal(await sends("force"),1);
  });

  it("keeps an unusable existing snapshot disabled until a later operation verifies it",async()=>{
    const existing={...headphones,snapshot:true};
    await open(existing.id,{lang:"en"},[existing,speakers]);
    const request=await forceRestore();
    await emit({...request,ok:true,returnedId:headphones.id,snapshotStatus:"unavailable",message:"Output rebuilt. Snapshot could not be verified."});
    await devices([existing,speakers],existing.id);
    assert.match(await content("notice"),/Output rebuilt/);
    assert.equal(await node("restore").isDisabled(),true);
    assert.match(await content("restore-help"),/diagnostic/i);
    await click("refresh");
    await devices([existing,speakers],existing.id);
    assert.equal(await node("restore").isDisabled(),true);
    await click("apply");
    await dialogButton("Apply settings").click();
    const apply=await latest();
    await emit({type:"operation",op:"apply",id:existing.id,requestId:apply.requestId,ok:true,message:"New settings verified"});
    await devices([existing,speakers],existing.id);
    assert.equal(await node("restore").isEnabled(),true);
    assert.equal(await sends("force"),1);
  });

  it("retains the force failure during refresh and requires a new confirmation to retry",async()=>{
    await open(headphones.id,{lang:"en"});
    const request=await forceRestore();
    const busy="The device was not removed. Stop playback and confirm again.";
    await emit({...request,ok:false,message:busy});
    assert.deepEqual(await latest(),{op:"refresh",id:headphones.id});
    await devices(endpoints,headphones.id);
    assert.match(await content("notice"),/Stop playback/);
    assert.equal(await visible("modal"),false);
    assert.equal(await sends("force"),1);
    const retry=await forceRestore();
    await emit({...retry,ok:false,message:"Removal returned 5. Check the output in Windows."});
    await devices([speakers],"");
    assert.equal(await node("output").inputValue(),"");
    assert.match(await content("notice"),/Removal returned 5/);
    assert.equal(await node("force").isDisabled(),true);
    assert.equal(await sends("force"),2);
  });

  it("offers independent rebuild after a failed snapshot restore only for the same verified card",async()=>{
    await open(speakers.id,{lang:"en"});
    async function failRestore() {
      await click("restore");
      await dialogButton("Restore factory defaults").click();
      const request=await latest();
      assert.equal(request.op,"restore");
      await emit({type:"operation",op:"restore",id:request.id,requestId:request.requestId,ok:false,message:"Fixture conflict"});
    }
    await failRestore();
    await devices(endpoints,speakers.id);
    assert.match(await content("modal"),/Snapshot restoration did not complete.*Fixture conflict/);
    assert.equal(await sends("force"),0);
    await dialogButton("Cancel").click();
    await failRestore();
    await devices([{...speakers,arrival:"new attachment"}],speakers.id);
    assert.equal(await visible("modal"),false);
    assert.equal(await sends("force"),0);
  });

  it("keeps hard restart beside both restore actions and gates it on the selected pending card", async () => {
    await open(headphones.id, {lang:"en"});
    await page.setViewportSize({width:360,height:720});
    for(const id of ["restore","hard-restart","force"]) {
      assert.equal(await visible(id), true);
      const box=await node(id).boundingBox();
      assert.ok(box.x>=0 && box.x+box.width<=360);
    }
    assert.equal(await node("hard-restart").isDisabled(), true);
    await devices([{...unsupported,restart_pending:true}],unsupported.id);
    assert.equal(await node("hard-restart").isDisabled(), true);
    await devices([{...headphones,restart_pending:true}],headphones.id);
    await click("hard-restart");
    const request=await latest();
    assert.equal(request.op,"prepareRestart");
    assert.equal(request.id,headphones.id);
    assert.equal(request.device.function_id,headphones.function_id);
    assert.ok(Number.isSafeInteger(request.requestId) && request.requestId>0);
    assert.equal(await dialogButton("Hard restart").count(),0);
    const reply={type:"restartPrepared",id:request.id,requestId:request.requestId,token:"fixture token",port:18};
    await emit({...reply,id:speakers.id});
    assert.equal(await dialogButton("Hard restart").count(),0);
    await emit(reply);
    assert.equal(await sends("hardRestart"),0);
    const body=await content("modal");
    for(const word of [/Fixture headphones/,/Fixture USB Audio/,/USB port\s+18/,/all playback and recording/,/pops/,/default output/]) assert.match(body,word);
    assert.equal(await dialogButton("Cancel").evaluate(n=>n===document.activeElement),true);
    await dialogButton("Hard restart").evaluate(n=>{n.click();n.click();});
    assert.equal(await sends("hardRestart"),1);
    assert.equal(await sends("force"),0);
    assert.equal(await sends("cancelRestart"),0);
    assert.deepEqual(await latest(),{...request,op:"hardRestart",token:reply.token});
    await dialogButton("Back to main window").click();
    assert.equal(await node("hard-restart").isDisabled(),true);
    const result={type:"operation",op:"hardRestart",requestId:request.requestId,id:request.id,ok:true,message:"Fixture reconnected"};
    await emit({...result,requestId:request.requestId+1});
    await emit({...result,type:"error",id:speakers.id});
    assert.equal(await node("force").isDisabled(),true);
    await emit(result);
    assert.equal((await latest()).op,"refresh");
    await devices();
    assert.equal(await node("hard-restart").isDisabled(),true);
    assert.match(await content("notice"),/Fixture reconnected/);
  });

  it("discards cancelled, expired and device-changed hard restart confirmations", async () => {
    const card={...headphones,restart_pending:true};
    await open(card.id,{},[card]);
    await page.clock.install();
    const begin=async()=>{
      await click("hard-restart");
      const {id,requestId}=await latest();
      return {type:"restartPrepared",id,requestId,token:"token-"+requestId,port:3};
    };
    const first=await begin();
    await dialogButton("取消").click();
    assert.deepEqual(await latest(),{op:"cancelRestart",id:card.id,requestId:first.requestId});
    const second=await begin();
    assert.notEqual(first.requestId,second.requestId);
    await emit(first);
    await emit({type:"error",op:"prepareRestart",id:card.id,requestId:first.requestId,message:"Stale preparation"});
    assert.equal(await dialogButton("硬重启").count(),0);
    await emit(second);
    assert.match(await content("modal"),/USB 端口\s+3/);
    assert.match(await content("modal"),/播放和录音.*爆音.*默认输出/);
    await page.evaluate(()=>document.dispatchEvent(new Event("visibilitychange")));
    await page.clock.fastForward(300);
    assert.equal((await latest()).op,"refresh");
    assert.equal(await dialogButton("硬重启").isDisabled(),true);
    await devices([card],card.id);
    assert.equal(await dialogButton("硬重启").isEnabled(),true);
    await dialogButton("硬重启").evaluate(n=>window.staleConfirm=n);
    await page.clock.fastForward(31000);
    assert.equal(await visible("modal"),false);
    assert.match(await content("notice"),/已过期/);
    const third=await begin();
    await emit(third);
    await page.evaluate(()=>window.staleConfirm.click());
    assert.equal(await visible("modal"),true);
    await page.keyboard.press("Escape");
    await emit(third);
    assert.equal(await visible("modal"),false);
    const fourth=await begin();
    await emit(fourth);
    await devices([{...card,arrival:"reconnected"}],card.id,true);
    assert.equal(await visible("modal"),false);
    const fifth=await begin();
    await devices([{...card,restart_pending:false}],card.id,true);
    await emit(fifth);
    assert.equal(await visible("modal"),false);
    assert.equal(await node("hard-restart").isDisabled(),true);
    assert.equal(await sends("hardRestart"),0);
  });

  it("recovers from a rejected port check or hard restart without sending another restart", async () => {
    const card={...headphones,restart_pending:true};
    await open(card.id,{},[card]);
    await click("hard-restart");
    let req=await latest();
    await emit({type:"error",...req,message:"Cannot resolve the USB port"});
    assert.equal(await visible("modal"),false);
    assert.match(await content("notice"),/Cannot resolve/);
    await click("hard-restart");
    req=await latest();
    await emit({type:"restartPrepared",...req,token:"token",port:0});
    assert.equal(await visible("modal"),false);
    assert.equal(await sends("hardRestart"),0);
    await click("hard-restart");
    req=await latest();
    await emit({type:"restartPrepared",...req,token:"token",port:7});
    await dialogButton("硬重启").click();
    await emit({type:"error",op:"hardRestart",id:card.id,requestId:req.requestId,message:"Card moved"});
    assert.equal(await visible("modal"),false);
    assert.match(await content("notice"),/Card moved/);
    assert.deepEqual(await latest(),{op:"refresh",id:card.id});
    await devices([card],card.id);
    assert.equal(await node("hard-restart").isEnabled(),true);
    assert.equal(await sends("hardRestart"),1);
  });

  it("offers one optional hard restart after a verified pending soft reload", async () => {
    await open(speakers.id);
    await click("apply"); await dialogButton("应用设置").click();
    const write=await latest();
    const result={type:"operation",op:write.op,id:write.device.id,requestId:write.requestId,
      offerRestart:true,ok:false,message:"Windows 3010"};
    await emit({...result,requestId:write.requestId+1});
    assert.equal((await latest()).op,"apply");
    await emit(result);
    assert.equal((await latest()).op,"refresh");
    assert.equal(await sends("prepareRestart"),0);
    const card={...speakers,min:96,restart_pending:true};
    await devices([card],card.id);
    const check=await latest();
    assert.equal(check.op,"prepareRestart");
    assert.match(await content("modal"),/普通重启后/);
    await emit({type:"restartPrepared",id:card.id,requestId:check.requestId,port:7,token:"fixture"});
    assert.match(await content("modal"),/USB 端口\s+7/);
    await dialogButton("取消").click();
    await emit(result);
    await click("refresh"); await devices([card],card.id);
    assert.equal(await visible("modal"),false);
    assert.equal(await node("hard-restart").isEnabled(),true);
    assert.equal(await sends("prepareRestart"),1);
    assert.equal(await sends("hardRestart"),0);
  });

  it("binds automatic restart offers to a pending write and the original connection", async () => {
    const card={...speakers,restart_pending:true};
    await open(card.id,{},[card]);
    const restore=async (result,refreshed=card)=>{
      await click("restore"); await dialogButton("恢复原厂默认设置").click();
      const write=await latest();
      await emit({type:"operation",op:"restore",id:card.id,requestId:write.requestId,...result});
      await devices([refreshed],refreshed.id);
    };
    assert.equal(await sends("prepareRestart"),0);
    await restore({ok:false,message:"Write failed"});
    assert.match(await content("modal"),/确认强制恢复/);
    await dialogButton("取消").click();
    await restore({ok:false,offerRestart:true},{...card,restart_pending:false});
    assert.match(await content("modal"),/确认强制恢复/);
    await dialogButton("取消").click();
    await restore({ok:false,offerRestart:true},{...card,arrival:"replugged"});
    await restore({ok:false,offerRestart:true},{...headphones,restart_pending:true});
    assert.equal(await visible("modal"),false);
    assert.equal(await sends("prepareRestart"),0);
    assert.equal(await sends("hardRestart"),0);
    assert.equal(await sends("force"),0);
  });

  it("measures a fixed-period non-UAC2 output read-only and can repeat the test", async () => {
    const card={...unsupported,name:"Built-in speakers",can_benchmark:true,snapshot:true,restart_pending:true,
      function_id:"",physical_id:"",rate:0,bits:0,raw_format:"",current:480,min:480,default:480,max:480};
    await open(card.id,{},[card]);
    assert.match(await content("binding"),/仅支持试流/);
    assert.match(await content("slider-help"),/不支持调整/);
    for(const id of ["apply","restore","hard-restart","force","slider","one","all"])
      assert.equal(await node(id).isDisabled(),true);
    await prepare(emptyDirectory);
    await dialogButton("选择目录").click();
    await emit(validDirectory);
    await dialogButton("开始测试").click();
    const request=await latest();
    assert.equal(request.op,"bench");
    assert.equal(request.device.id,card.id);
    assert.match(await content("modal"),/设备公布下限/);
    await emit({type:"result",deviceId:card.id,data:{restored:true,currentMs:18,defaultMs:18,
      defaultComparison:"samePeriod",sevenComparison:"unavailable",sevenDelta:77,
      bands:{current:{periodMs:10,queueMs:18},default:{periodMs:10,queueMs:18},seven:{reason:"No 7 ms period"}}}});
    await devices([card],card.id);
    assert.match(await content("comparisons"),/同一周期/);
    assert.match(await content("comparisons"),/无法比较/);
    assert.doesNotMatch(await content("comparisons"),/节省|增加/);
    await node("test-details").locator("summary").click();
    assert.match(await content("test-data"),/设备公布下限/);
    await start();
    assert.equal(await sends("bench"),2);
    for(const op of ["apply","restore","force","prepareRestart","hardRestart"]) assert.equal(await sends(op),0);
  });

  it("requires a usable shared mix and period range even when a driver permits testing", async () => {
    const card={...unsupported,id:"hdmi",name:"HDMI output",can_benchmark:true};
    await open(card.id,{lang:"en"},[card]);
    const set=patch=>devices([{...card,...patch}],card.id);
    for(const bad of [{can_benchmark:false},{can_benchmark:undefined},{mix_rate:0},{mix_channels:0},
        {mix_bits:0},{mix_encoding:""},{min:0},{fundamental:0},{max:card.min-1},{default:card.min-1}]) {
      await set(bad);
      assert.equal(await node("bench").isDisabled(),true);
    }
    await set({});
    await prepare();
    await set({can_benchmark:false});
    assert.equal(await visible("modal"),false);
    assert.equal(await sends("bench"),0);
    await set({});
    await prepare();
    await dialogButton("Start test").click();
    await emit({type:"result",deviceId:card.id,data:{restored:true,currentMs:11,sevenComparison:"difference",sevenDelta:23}});
    await set({});
    assert.match(await content("comparisons"),/Unavailable/);
    assert.doesNotMatch(await content("comparisons"),/Saved|Increased/);
    assert.match(await content("binding"),/Playback test only/);
    assert.match(await content("slider-help"),/adjustment is unavailable/);
  });

  it("selects and validates a folder without starting a test until one explicit Start", async () => {
    await page.setViewportSize({width: 600, height: 520});
    await open();
    await prepare(emptyDirectory);
    await checkCompactPreparation();
    assert.match(await content("modal"), /osu!（lazer）安装文件夹，也可以选择其中的 current 文件夹/);
    assert.match(await content("modal"), /BASS 和 BASSWASAPI.*混好的声音在队列里要等多久/);
    assert.match(await content("modal"), /跳过了游戏逻辑和混音.*按键到实际出声/);
    assert.match(await content("modal"), /全零音频数据，系统音量和静音状态保持不变/);
    assert.equal(await dialogButton("开始测试").isDisabled(), true);
    await dialogButton("选择目录").click();
    await emit({type: "directory", ok: false, path: "X:\\Wrong", message: "Missing basswasapi.dll"});
    assert.match(await content("modal"), /Missing basswasapi\.dll/);
    assert.match(await content("modal"), /X:\\Wrong/);
    await dialogButton("重新选择目录").click();
    await emit({type: "directory", ok: false, cancelled: true});
    assert.equal(await dialogButton("开始测试").isDisabled(), true);
    await dialogButton("重新选择目录").click();
    await emit(validDirectory);
    assert.match(await content("modal"), /已就绪/);
    assert.ok((await content("modal")).includes(displayPath));
    assert.equal((await content("modal")).includes(nativePath), false);
    await dialogButton("取消").click();
    assert.equal(await sends("bench"), 0);
    await prepare();
    await dialogButton("重新选择目录").click();
    await emit({type: "directory", ok: false, cancelled: true});
    assert.equal(await dialogButton("开始测试").isEnabled(), true);
    assert.equal(await sends("bench"), 0);
    await dialogButton("开始测试").evaluate(b => {b.click(); b.click();});
    assert.equal(await sends("bench"), 1);
  });

  it("keeps English, light mode and saved-folder verification independent of playback", async () => {
    await page.setViewportSize({width: 600, height: 520});
    await open(speakers.id, {lang: "en", theme: "light", hasDir: true, path: nativePath});
    assert.equal(await page.locator("html").getAttribute("lang"), "en");
    assert.equal(await page.locator("html").getAttribute("data-theme"), "light");
    assert.equal(await node("refresh").getAttribute("title"), "Refresh Windows readings");
    await prepare();
    await checkCompactPreparation();
    assert.match(await content("modal"), /osu! \(lazer\) installation folder or its current subfolder/);
    assert.match(await content("modal"), /BASS and BASSWASAPI.*mixed audio.*event-driven WASAPI shared output/);
    assert.match(await content("modal"), /skips game logic and mixing.*keypress to audible sound/);
    assert.match(await content("modal"), /zero-valued samples.*volume and mute unchanged/);
    assert.equal(await dialogButton("Start test").isEnabled(), true);
    await dialogButton("Cancel").click();
    await page.setViewportSize({width: 1120, height: 780});
    await click("zh"); await click("dark");
    assert.deepEqual(await latest(), {op: "prefs", theme: "dark", lang: "zh"});
    assert.equal(await sends("bench"), 0);
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
    assert.equal(await page.locator("footer").evaluate(n => n.getBoundingClientRect().bottom <= innerHeight), true);
    await page.setViewportSize({width: 760, height: 690});
    assert.equal(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), true);
  });

  it("explains file signatures and interface compatibility without blocking accepted library updates", async () => {
    await open(headphones.id, {lang: "en"});
    const rejected = {type: "directory", checked: true, ok: false, path: nativePath};
    await prepare({...rejected, message: "bass.dll：Windows 签名验证失败（0x800B0100）。；basswasapi.dll：签名发布者不是 Un4seen Developments Ltd。"});
    let explanation = await content("modal");
    assert.match(explanation, /bass\.dll.*could not verify the digital signature.*0x800B0100/);
    assert.match(explanation, /basswasapi\.dll.*signer is not Un4seen Developments Ltd/);
    assert.equal(await dialogButton("Start test").isDisabled(), true);
    await dialogButton("Choose another folder").click();
    await emit({type: "directory", ok: false, path: nativePath, message: "basswasapi.dll：无法读取已验证的签名发布者。"});
    assert.match(await content("modal"), /could not identify the publisher/);
    await dialogButton("Choose another folder").click();
    await emit({...validDirectory, versions: {bass: "2.4.99.1", basswasapi: "2.4.99.2"}});
    assert.equal(await dialogButton("Start test").isEnabled(), true);
    assert.equal(await sends("bench"), 0);
    await dialogButton("Start test").click();
    await emit({type: "error", op: "bench", message: "basswasapi.dll 缺少接口 BASS_WASAPI_GetInfo。"});
    await devices();
    assert.match(await content("modal"), /basswasapi\.dll is missing the required function BASS_WASAPI_GetInfo/);
    await dialogButton("Cancel").click();
    await click("zh");
    await prepare({...rejected, message: "bass.dll：Windows 签名验证失败（0x800B0100）。"});
    explanation = await content("modal");
    assert.match(explanation, /bass\.dll：Windows 签名验证失败（0x800B0100）/);
    assert.equal(await dialogButton("开始测试").isDisabled(), true);
  });

  it("reports real progress, preserves it while hidden, and does not count skipped rounds as samples", async () => {
    await open();
    await start();
    assert.match(await content("modal"), /已完成 0 \/ 30 轮/);
    const {requestId}=await latest();
    const progress=(stage,done,skipped=0)=>emit({type:"progress",deviceId:headphones.id,requestId,stage,done,skipped,total:30});
    await emit({type:"progress",stage:"current",done:27,total:30,deviceId:speakers.id,requestId});
    await emit({type:"progress",stage:"current",done:27,total:30,deviceId:headphones.id,requestId:requestId+1});
    assert.match(await content("modal"), /已完成 0 \/ 30 轮/);
    await progress("default",10);
    assert.match(await content("modal"), /10 \/ 30/);
    await dialogButton("返回主界面").click();
    await click("bench");
    assert.match(await content("modal"), /10 \/ 30/);
    // The native worker reports a cumulative skipped count separately from actual samples.
    await progress("seven",10,10);
    await progress("current",11,10);
    const bands = node("modal").locator(".progress-bands .band");
    assert.match(await bands.nth(1).innerText(), /已跳过/);
    assert.match(await bands.nth(2).innerText(), /1 \/ 10/);
    await progress("current",20,10);
    assert.match(await content("modal"), /已完成 20 \/ 30 轮 · 已跳过 10 轮/);
    assert.equal(await node("modal").locator("progress").getAttribute("value"),"30");
    await dialogButton("取消测试").click();
    assert.equal((await latest()).op, "cancelBench");
    await emit({type: "result", deviceId: headphones.id, data: {
      cancelled: true, restored: false, reason: "Stopped", defaultComparison: "samePeriod",
      sevenComparison: "unavailable", bands: {seven: {rounds:[], reason: "No independent reference"}},
    }});
    await devices();
    assert.match(await content("test-state"), /已取消/);
    assert.match(await content("test-state"), /待复核/);
    assert.match(await content("comparisons"), /同一周期/);
    assert.match(await content("comparisons"), /无法比较/);
    await node("test-details").locator("summary").click();
    assert.match(await content("test-data"),/已跳过 10 轮/);
  });

  it("shows ten rounds per supported band and distinguishes completed from valid rounds",async()=>{
    await open(speakers.id,{lang:"en"});
    await prepare();
    assert.match(await content("modal"),/10 rounds.*30 in total/);
    await dialogButton("Start test").click();
    const {requestId}=await latest();
    const stage=async(stage,done)=>emit({type:"progress",stage,done,total:30,deviceId:speakers.id,requestId});
    const bands=node("modal").locator(".progress-bands .band");
    await stage("default",10);
    await stage("seven",19);
    assert.match(await bands.nth(1).innerText(),/9 \/ 10/);
    await stage("current",30);
    for(const band of await bands.all()) assert.match(await band.innerText(),/10 \/ 10/);
    assert.match(await content("modal"),/Completed 30 \/ 30 rounds/);
    const rounds=Array.from({length:10},()=>({frames:480,queueMs:10}));
    await emit({type:"result",requestId,deviceId:speakers.id,data:{restored:true,bands:{default:{rounds},seven:{rounds:[...rounds.slice(0,9),{reason:"No queue samples"}]}}}});
    await devices(endpoints,speakers.id);
    await node("test-details").locator("summary").click();
    assert.match(await content("test-data"),/Valid rounds 10 \/ 10/);
    assert.match(await content("test-data"),/Valid rounds 9 \/ 10/);
  });

  it("keeps measured queue results separate from applied periods and handles explicit 10 ms comparisons", async () => {
    const device = {...speakers, default: 384};
    await open(device.id, {}, [device]);
    await start();
    await emit({type: "result", deviceId: device.id, data: {
      explicitTen: true, currentMs: 12, defaultMs: 36, sevenMs: 10.76,
      defaultDelta: 24, sevenDelta: -1.24, defaultComparison: "difference",
      sevenComparison: "difference", restored: true, versions: {bass: "fixture"},
      bands: {default: {periodMs: 10, queueMs: 36}, seven: {periodMs: 7, queueMs: 10.76}, current: {periodMs: 3, queueMs: 12}},
    }});
    await devices([device], device.id);
    assert.equal(await visible("modal"), false);
    assert.equal(await content("queue-ms"), "~12.0");
    assert.match(await content("read-list"), /8\.000 ms/);
    assert.match(await content("comparisons"), /较 10 ms 对照/);
    assert.match(await content("comparisons"), /节省 ~24\.0 ms/);
    assert.match(await content("comparisons"), /增加 ~1\.2 ms/);
    await node("test-details").locator("summary").click();
    assert.match(await content("test-data"), /队列中位数 ~10\.8 ms/);
    assert.match(await content("test-data"), /10 ms 对照/);
    await devices([{...device, bits: 16}], device.id, true);
    assert.match(await content("test-state"), /已变化/);
    await devices([device], device.id, true);
    assert.match(await content("test-state"), /已变化/);
    assert.match(await content("test-context"), /测试时下限/);
  });

  it("requires a fresh explicit start after a runtime DLL error and successful retry", async () => {
    await open();
    await start();
    await emit({type: "error", op: "bench", message: "bass.dll is not x64"});
    assert.match(await content("modal"), /bass\.dll is not x64/);
    await devices();
    await dialogButton("重新选择目录").click();
    await emit(validDirectory);
    assert.equal(await sends("bench"), 1);
    await dialogButton("开始测试").click();
    assert.equal(await sends("bench"), 2);
  });

  it("preserves same-format drafts and rejects outdated confirmation and preparation buttons", async () => {
    await open(speakers.id);
    await node("slider").fill("4.2");
    await click("all");
    await click("apply");
    await dialogButton("应用设置").evaluate(n => {window.staleApply = n;});
    await devices([{...headphones, bits: 16}, speakers], headphones.id, true);
    assert.equal(await node("output").inputValue(), speakers.id);
    assert.equal(await visible("modal"), true);
    await devices([{...speakers, min: 192}], speakers.id, true);
    assert.equal(await visible("modal"), false);
    assert.equal(await content("ms"), "4.200");
    assert.equal(await node("all").getAttribute("aria-pressed"), "true");
    await devices([speakers], speakers.id, true);
    await page.evaluate(() => window.staleApply.click());
    assert.equal(await sends("apply"), 0);
    await prepare();
    await dialogButton("开始测试").evaluate(n => {window.staleStart = n;});
    const replacement = {...speakers, bits: 16, rate: 96000, mix_rate: 96000,
      min: 288, default: 960, max: 960, current: 960, current_rate: 96000};
    await devices([replacement], speakers.id, true);
    assert.equal(await visible("modal"), false);
    assert.match(await content("notice"), /确认已失效/);
    assert.match(await content("format"), /16 位 PCM · 96 kHz/);
    assert.match(await content("mix"), /96 kHz/);
    await page.evaluate(() => window.staleStart.click());
    assert.equal(await sends("bench"), 0);
    await click("en");
    assert.match(await content("notice"), /output or format changed/i);
  });

  it("never redirects to another output while reconnecting or while a read is pending", async () => {
    await open(speakers.id, {lang: "en"});
    await devices([headphones], "", true);
    assert.equal(await node("output").inputValue(), "");
    assert.match(await content("notice"), /offline/);
    await devices([headphones], headphones.id, true);
    assert.equal(await node("output").inputValue(), "");
    await devices(endpoints, speakers.id, true);
    assert.equal(await node("output").inputValue(), speakers.id);
    await click("refresh");
    await devices([{...speakers, rate: 96000, mix_rate: 96000}], speakers.id, true);
    assert.match(await content("format"), /48 kHz/);
    assert.equal(await node("refresh").isDisabled(), true);
    await devices(endpoints, speakers.id);
    const notice = await content("notice");
    await emit({type: "error", automatic: true, message: "Failed background read"});
    assert.equal(await content("notice"), notice);
    assert.equal(await visible("modal"), false);
  });
});

