"use strict";

const words = {
  about:["关于","About"], diagnostics:["诊断探针","Diagnostic probe"],
  windowMinimize:["最小化","Minimize"], windowMaximize:["最大化","Maximize"], windowRestore:["还原窗口","Restore window"], closeWindow:["关闭窗口","Close window"],
  repository:["GitHub 源码","Source on GitHub"],
  aboutPurpose:["调整使用微软 USB Audio 2.0 驱动的声卡在共享播放时的最小周期，并通过静音试流测量输出排队延迟。","Adjust the minimum shared playback period for devices using the Microsoft USB Audio 2.0 driver and measure output queue latency with a silent playback test."],
  aboutLicense:["许可 · PolyForm Noncommercial 1.0.0","License · PolyForm Noncommercial 1.0.0"],
  probeHelp:["如果修改未生效，请为下方所选播放输出生成报告。打开 GitHub Issue，填写声卡品牌、型号、执行过的操作及实际结果。请先查看报告，再手动附上文件。","If a change did not take effect, generate a report for the playback output shown below. Open a GitHub issue and include the sound card make and model, the steps you took, and what happened. Review the report, then attach the file manually."],
  reportLocation:["报告位置（EXE 同级）","Report location (beside the EXE)"], generate:["生成报告","Generate report"], openIssue:["打开 GitHub Issue","Open GitHub issue"],
  generating:["正在读取所选设备并生成报告…","Reading the selected device and generating the report…"],
  generated:["报告已保存。再次生成会覆盖这份文件。","Report saved. Generating it again replaces this file."],
  noTarget:["请先在主界面选择一个活动播放输出。","Select an active playback output in the main window first."],
  reportFailed:["生成失败","Could not generate the report"], linkFailed:["无法用普通用户的默认浏览器打开链接","Could not open the link in the desktop user’s default browser"],
  linkOpened:["已请求默认浏览器打开链接。","Sent the link to your default browser."],
  snapshotStorage:["恢复快照未能完整载入，已暂停修改周期、写回快照和硬重启。仍可由 Windows 重建已核对声卡的原厂设置，也可读取设备、试流及生成诊断报告。","Recovery snapshots could not be loaded. Changing periods, writing snapshots and hard restarts are disabled. Windows can still rebuild defaults for a verified card. Readings, playback tests and diagnostic reports remain available."],
  preferenceStorage:["未能读取原有同意记录或偏好设置。可仅为本次会话同意风险并继续。","Saved consent or preferences could not be read. You can accept the risks for this session."],
  sessionAccept:["仅本次同意并继续","Accept for this session"], sessionConsent:["仅本次会话已同意风险。下次打开时需重新确认。","You accepted the risks for this session. You will be asked again next time."],
  missingRange:["无法完整核对此端点的周期能力记录。","The period capability records for this output could not be verified."],
  offerRebuild:["可由 Windows 重新识别这块声卡并重建默认设置。","Windows can rediscover this card and rebuild its default settings."],
  failedSnapshot:["写回快照未能完成。","Snapshot restoration did not complete."],
  app:["UAC2 共享周期","UAC2 Shared Period"],
  output:["播放输出","Playback output"], driver:["驱动绑定","Driver binding"], format:["设备格式","Device format"], mix:["共享混音","Shared mix"],
  supported:["微软 UAC 2.0 已核对","Microsoft UAC 2.0 verified"], unsupported:["不支持","Unsupported"], unknownDriver:["无法确认驱动","Driver unverified"],
  testOnly:["仅支持试流","Playback test only"], noAdjustment:["此驱动不支持调整共享周期。","Period adjustment is unavailable for this driver."],
  deviceMinimum:["设备公布下限","Device minimum"], minimumQueue:["设备公布下限的输出排队延迟","Queue latency at the device minimum"],
  prepareReadOnly:["点击「开始测试」后，每个支持的档位测量 10 轮，三档共 30 轮。试流仅发送全零音频数据，系统音量和静音状态保持不变。","Click Start test to measure 10 rounds per supported period setting, up to 30 in total. The stream sends only zero-valued samples and leaves system volume and mute unchanged."],
  pick:["请选择播放输出","Select a playback output"], noDevice:["未找到活动播放输出","No active playback outputs"], missing:["未读到","Unavailable"],
  bit:["位","bit"], float:["浮点","float"], channels:["声道","channels"], container:["容器","container"],
  target:["目标最小周期","Target minimum period"], targetA11y:["目标最小周期（毫秒）","Target minimum period in milliseconds"], frames:["帧","frames"],
  rounded:["取整后","Rounded"], sliderHelp:["根据自身设备能力和实际听感，合理选择合适的缓冲数值","Choose a buffer value suited to your device and listening experience."],
  low:["此设备可能无法稳定播放，或出现断音、爆音、无声。请先降低音量，如有异常可调高缓冲。","This device may stutter, pop, or fall silent. Lower the volume first and increase the buffer if playback becomes unstable."],
  scope:["生效范围","Sample rate scope"], one:["当前采样率","Current sample rate"], all:["全部采样率","All sample rates"], apply:["应用设置","Apply settings"],
  windows:["Windows 实读","Windows readings"], refresh:["刷新 Windows 实读","Refresh Windows readings"], refreshing:["正在读取…","Reading…"], readAt:["读取于","Read at"], lastRead:["上次读取","Last read"],
  minimum:["已公布下限","Published minimum"], default:["默认周期","Default period"], current:["当前周期","Current period"],
  minHelp:["此格式下应用可请求的最小周期","Shortest period apps can request in this format"],
  defaultHelp:["Windows 在此格式下的默认周期","Windows default for this format"], currentHelp:["读取时共享引擎实际使用的周期","Period the shared engine was using when read"],
  queue:["输出排队延迟","Output queue latency"], test:["试流验证","Test playback"], testing:["查看测试进度","View progress"], currentSetting:["当前设置的输出排队延迟","Queue latency with applied settings"],
  queueHelp:["新声音送入后，前面已排队的音频大约还要播放多久。","How long the audio already queued ahead of a new sound takes to play."],
  noTest:["尚未试流","No measurement yet"], noTestHelp:["试流会在所选输出播放静音，每个支持的周期档位测量 10 轮。","Run a silent playback test on this output to measure 10 rounds per supported period setting."],
  previous:["上次试流","Last test"], stale:["设备、格式或设置已变化。以下是上次测试条件下的结果，请重新试流。","The device, format, or settings changed. These results are from the previous conditions. Run the test again."],
  ten:["10 ms 对照","10 ms reference"], vsTen:["较 10 ms 对照","Vs. 10 ms reference"],
  minimumAtTest:["测试时下限","Minimum at test"], vsDefault:["较默认","Vs. default"], vsSeven:["较 7 ms 对照","Vs. 7 ms reference"], saved:["节省","Saved"], increased:["增加","Increased"],
  equal:["约相同","About the same"], same:["同一周期","Same period"], unavailable:["无法比较","Unavailable"], unmeasured:["待测","Untested"],
  details:["查看各档实测与条件","Measurement details"], bandCurrent:["当前最小周期","Applied minimum"], seven:["7 ms 对照","7 ms reference"], actual:["实际周期","Actual period"], queueMedian:["队列中位数","Queue median"],
  verified:["已复核测试前设置","Pre-test settings verified"], pending:["测试前设置待复核","Pre-test settings pending verification"], cancelled:["测试已取消","Test cancelled"], library:["音频库","Audio libraries"],
  recovery:["恢复设置","Restore settings"], restore:["恢复原厂默认设置","Restore factory defaults"], force:["强制恢复","Force restore"],
  hardRestart:["硬重启","Hard restart"], restartHelp:["普通重启后仍需重新连接声卡时可用。","Available when the card still needs reconnection after a normal restart."],
  confirmRestart:["确认硬重启","Confirm hard restart"], restartChecking:["正在核对此声卡的 USB 端口…","Checking this card’s USB port…"], usbPort:["USB 端口","USB port"],
  restartSuggested:["普通重启后，设置仍待生效。是否现在重新连接这块声卡？","The settings are still pending after a normal restart. Reconnect this card now?"],
  restartText:["请先暂停播放并降低音量。硬重启会让 Windows 断开并重新连接已核对的 USB 端口，整块声卡的播放和录音都会中断，重连时可能产生爆音。Windows 可能切换默认输出，完成后请核对。","Pause playback and lower the volume first. Windows will disconnect and reconnect the USB port shown here. This interrupts all playback and recording on the card and may cause loud pops. Windows may switch the default output. Check it after reconnection."],
  restartExpired:["硬重启确认已过期，请重新核对 USB 端口。","Hard restart confirmation expired. Check the USB port again."],
  restartInvalid:["未能核对所选声卡的 USB 端口，请重新读取设备。","Could not verify the USB port for the selected card. Refresh the device readings."],
  noSnapshot:["此输出没有匹配的原厂快照。","No matching factory snapshot for this output."], noSupport:["所选输出尚未核对为微软 UAC 2.0。","Select a verified Microsoft UAC 2.0 output."],
  theme:["外观","Appearance"], dark:["暗色主题","Dark theme"], light:["亮色主题","Light theme"],
  safety:["使用前请留意","Before you start"], safetyText:["调整音频缓冲或重新连接设备可能导致播放中断、爆音，严重时可能损坏音频设备或损伤听力。请先暂停播放、降低音量。是否继续使用？","Adjusting the audio buffer or reconnecting a device can interrupt playback or cause loud pops that may damage equipment or hearing. Pause playback and lower your volume first. Do you want to continue?"],
  yes:["是，继续使用","Yes, continue"], no:["否，退出","No, exit"], cancel:["取消","Cancel"], close:["关闭","Close"], hide:["返回主界面","Back to main window"],
  confirmApply:["确认应用设置","Apply these settings?"], confirmRestore:["确认恢复原厂默认设置","Restore factory defaults?"], confirmForce:["确认强制恢复","Confirm force restore"],
  applyText:["将为所选输出设置应用可请求的最小周期，保留对应格式原有的默认周期。请先暂停播放并降低音量，设备重启可能中断同卡音频或产生爆音。","Set the minimum period apps can request on this output and keep each format’s original default period. Pause playback and lower the volume. Restarting the device can interrupt other audio on this card or cause pops."],
  restoreText:["将写回本工具首次修改此输出前保存的设置。该快照可能包含当时已有的自定义设置。请先暂停播放并降低音量，设备重启可能中断同卡音频或产生爆音。","Restore this output’s snapshot from before the first change made by this tool. It may include custom settings that were already present. Pause playback and lower the volume. Restarting the device can interrupt audio on this card or cause pops."],
  forceText:["将卸载此声卡的 USB 音频设备实例，保留微软驱动包，再让 Windows 重新识别声卡并重建默认设置。如声卡未返回，将对已核对的 USB 端口尝试一次软件重连。同卡所有播放和录音端点都可能中断，重连可能产生爆音。Windows 可能切换默认输出或重置音量，必要时需重新插拔或重启电脑。请先暂停播放并降低音量。确认强制恢复吗？","Windows will remove this card’s USB audio device instance, keep the Microsoft driver package, and rediscover the card to rebuild its defaults. If the card does not return, the tool will try one software reconnection on the verified USB port. All playback and recording endpoints on the card may be interrupted. Reconnection may cause pops, switch your default output, or reset volume. You may need to reconnect the card or restart Windows. Pause playback and lower the volume. Continue with force restore?"],
  usb:["USB 音频功能","USB audio function"], affected:["同卡可能受影响的端点","Endpoints that may be interrupted"], targetScope:["目标与范围","Target and scope"],
  operating:["正在处理设备设置","Updating device settings"], operatingText:["正在等待 Windows 重新识别设备并读回设置。返回主界面后，操作仍会继续。","Waiting for Windows to rediscover the device and read back its settings. You can return to the main window while this runs."],
  changed:["输出或格式已变化，本次确认已失效。请重新核对设备。","The output or format changed. Review the device and confirm again."],
  formatChanged:["已检测到输出或格式变化，Windows 实读已更新。","The output or format changed. Windows readings have been updated."],
  dirTitle:["试流准备","Prepare playback test"], dirText:["请选择 osu!（lazer）安装文件夹，也可以选择其中的 current 文件夹。试流需要由 Un4seen Developments Ltd 签名的 64 位 bass.dll 和 basswasapi.dll。","Select the osu! (lazer) installation folder or its current subfolder. The test needs 64-bit copies of bass.dll and basswasapi.dll signed by Un4seen Developments Ltd."],
  testPurpose:["试流使用本机 osu!（lazer）的 BASS 和 BASSWASAPI 音频库，模拟游戏通过 WASAPI 共享事件模式输出时，混好的声音在队列里要等多久。独立的静音流跳过了游戏逻辑和混音，只估算输出队列等待时间，无法测出从按键到实际出声的总延迟。","The test uses BASS and BASSWASAPI from your local osu! (lazer) installation to estimate how long a game’s mixed audio would wait when using event-driven WASAPI shared output. A separate silent stream skips game logic and mixing, so the result covers only the output queue and cannot give the total time from a keypress to audible sound."],
  dirLabel:["音频库目录","Library folder"], noDir:["尚未选择","No folder selected"], pathMissing:["未取得目录路径","Folder path unavailable"], dirReady:["已就绪","Ready"], dirChecking:["正在检查音频库…","Checking audio libraries…"],
  start:["开始测试","Start test"], prepareHelp:["点击「开始测试」后，将按已生效的设置测量，每个支持的档位 10 轮，三档共 30 轮。试流仅发送全零音频数据，系统音量和静音状态保持不变。","Click Start test to measure 10 rounds per supported period setting with the applied configuration, up to 30 in total. The stream sends only zero-valued samples and leaves system volume and mute unchanged."],
  choose:["选择目录","Choose folder"], rechoose:["重新选择目录","Choose another folder"], libraryError:["无法使用此目录的音频库","Audio libraries unavailable"],
  progress:["试流验证","Playback test"], prepare:["核对设备与音频库","Checking device and libraries"], measuring:["正在测量","Measuring"], verify:["复核测试前设置","Verifying pre-test settings"], complete:["测试完成","Test complete"], rounds:["轮","rounds"],
  finishedRounds:["已完成","Completed"], validRounds:["有效轮数","Valid rounds"],
  cancelBench:["取消测试","Cancel test"], cancelling:["正在取消并复核…","Cancelling and verifying…"], verifyingHelp:["结束或取消后，关闭静音流并复核测试前的设置。","After completion or cancellation, the silent streams close and the original settings are checked."],
  retryRead:["读取失败，可再次刷新。","Read failed. Refresh to try again."], opError:["操作未完成","Operation incomplete"], done:["操作已完成","Operation complete"],
  unknownEndpoints:["未取得完整端点名称","Full endpoint names unavailable"], skipped:["已跳过","Skipped"], wait:["等待读取","Awaiting reading"],
  partial:["Windows 未返回完整周期读数。","Windows did not return all three period readings."],
  offline:["所选播放输出已离线。请重新选择输出。","The selected playback output went offline. Select an output again."],
};
const $ = id => document.getElementById(id);
const testBands=["default","seven","current"];
const roundsPerBand=10;
const state = {lang:"zh", theme:"dark", accepted:false, initialized:false, devices:[], id:"",offlineId:"", target:3, dirty:false, all:false, hasDir:false,dirPath:"",checkingDir:false,choosingDir:false,
  maximized:false, version:"", diagnosticPath:"", reportRequest:null, reportResult:null,
  migrationError:"", preferencesError:"", sessionConsent:false, consentError:"", rebuildOffer:null,
  stamp:null, readsFailed:false, reading:false, notice:"", pending:null, job:null, restartOffer:null, run:null, dialog:null, results:new Map(), lastResult:null};
const t = key => words[key]?.[state.lang === "en" ? 1 : 0] ?? key;
const el = (tag, value, cls) => {
  const node = document.createElement(tag);
  if (value != null) node.textContent = value;
  if (cls) node.className = cls;
  return node;
};
function send(op, payload = {}) {
  window.ipc.postMessage(JSON.stringify({op,...payload}));
}
function title(node, value) { node.title=value; node.setAttribute("aria-label",value); }
function positive(value) { return Number.isFinite(value) && value > 0; }
function valid(value) { return Number.isFinite(value); }
function selected() {return state.devices.find(device => device.id === state.id);}
function formatKey(d) {
  if (!d) return "";
  return JSON.stringify([d.id,d.function_id,d.physical_id,d.container,d.arrival,d.driver_version,d.association,d.form_factor,d.raw_format,d.rate,d.bits,d.channels,d.encoding,d.mix_raw,d.mix_rate,d.mix_bits,d.mix_channels,d.mix_encoding]);
}
function conditions(d) {return d ? JSON.stringify([formatKey(d),d.min,d.default,d.fundamental,d.max,d.can_benchmark]) : "";}
function identify(d) {
  return d ? JSON.stringify([conditions(d),d.supported,d.can_rebuild,d.snapshot,d.siblings]) : "";
}
function rebuildKey(d) {
  return d ? JSON.stringify([d.id,d.function_id,d.physical_id,d.container,d.arrival,d.driver_version,d.association,d.siblings,isRebuildTarget(d)]) : "";
}
function confirmationIdentity(op,d) {return op==="force"?rebuildKey(d):identify(d);}
function rateText(rate) {return positive(rate) ? (rate / 1000).toLocaleString("en-US",{maximumFractionDigits:3}) + " kHz" : "";}
function fmt(d, mix=false) {
  if (!d) return t("missing");
  const rate = mix ? d.mix_rate : d.rate;
  const bits = mix ? d.mix_bits : d.bits;
  const enc = String((mix ? d.mix_encoding : d.encoding) || "");
  const codec = /float|浮点/i.test(enc) ? t("float") : enc;
  const pcm = [positive(bits) ? bits + " " + t("bit") : "",codec].filter(Boolean).join(" ");
  return [pcm,rateText(rate)].filter(Boolean).join(" · ") || t("missing");
}
function formatDetails(d,mix=false) {
  if (!d) return t("missing");
  const count=mix ? d.mix_channels : d.channels;
  const container=mix ? d.mix_container_bits : d.container_bits;
  const bits=mix ? d.mix_bits : d.bits;
  return [fmt(d,mix),positive(count) ? count+" "+t("channels") : "",positive(container) && container!==bits ? container+" "+t("bit")+" "+t("container") : ""].filter(Boolean).join(" · ");
}
function period(frames,rate) {return positive(frames) && positive(rate) ? frames*1000/rate : null;}
function engineRate(d) {return d?.mix_rate;}
function shortMs(value) {return valid(value) ? Number(value.toFixed(3))+" ms" : t("missing");}
function realMs(value) {return valid(value) ? value.toFixed(3)+" ms" : t("missing");}
function approx(value) {return "~"+value.toFixed(1)+" ms";}
function option(value,label) {
  const n=el("option",label);
  n.value=value;
  if(value) n.id="endpoint-"+value;
  return n;
}
function setPrefs() {send("prefs",{theme:state.theme,lang:state.lang});}
function setLanguage(lang) {state.lang=lang;setPrefs();render();if(state.dialog) drawDialog();}
function deviceName(d){return d?.name || t("missing");}
function refresh(id=state.id || state.offlineId) {
  if (!state.accepted || state.reading) return;
  state.reading=true;
  if(state.dialog?.kind==="restart") drawDialog();
  send(id ? "refresh" : "list",id ? {id} : {});
  render();
}
function sliderMax(d) {
  const limit=period(d?.default,engineRate(d));
  return valid(limit) ? Math.min(10,Math.floor((limit+1e-8)*10)/10) : 10;
}
function draftFor(d) {
  state.dirty=false; state.all=false;
  const ms=period(d?.min,engineRate(d));
  state.target=valid(ms) ? Math.max(.5,Math.min(sliderMax(d),Math.round(ms*10)/10)) : 3;
}
function idle() {return !state.job && !state.run && !state.reportRequest && !state.readsFailed && !state.reading;}
function allowed(d) {return d?.supported===true && !state.migrationError && idle();}
function isRebuildTarget(d) {return (d?.can_rebuild ?? d?.supported)===true;}
function canRebuild(d) {
  return isRebuildTarget(d) && !state.job && !state.run && !state.reportRequest && !state.reading;
}
function fallbackReason(d) {
  if(state.migrationError) return t("snapshotStorage");
  if(!d?.supported || state.readsFailed || !positive(d.min) || !positive(d.default) || !positive(d.fundamental) || !positive(d.max))
    return t("missingRange");
  return !d.snapshot ? t("noSnapshot") : "";
}
function testable(d) {
  const capable=d?.can_benchmark===true || (d?.can_benchmark===undefined && d?.supported===true);
  return capable && positive(d.mix_rate) && positive(d.mix_bits) && positive(d.mix_channels) && !!d.mix_encoding
    && positive(d.min) && positive(d.fundamental) && positive(d.default) && positive(d.max)
    && d.default>=d.min && d.max>=d.default;
}
function currentBand(d) {return d && !d.supported ? "deviceMinimum" : "bandCurrent";}
function drawTarget(d) {
  const slider=$("slider"), max=Math.max(.5,sliderMax(d));
  slider.max=String(max);
  slider.value=String(state.target);
  slider.style.setProperty("--fill", (max>.5 ? (state.target-.5)/(max-.5)*100 : 0)+"%");
  $("ms").textContent=d?.supported ? state.target.toFixed(3) : "—";
  $("ms").nextElementSibling.hidden=!d?.supported;
  const rate=d?.rate, step=d?.fundamental;
  const count=d?.supported && positive(rate) && positive(step) ? Math.ceil((Math.round(state.target*10)*rate)/(10000*step))*step : null;
  $("frames").textContent=count ? count+" "+t("frames") : t("missing");
  $("rounded").textContent=count && Math.abs(period(count,rate)-state.target)>.0005 ? t("rounded")+" "+realMs(period(count,rate)) : "";
  slider.setAttribute("aria-valuetext",shortMs(state.target));
  $("low").hidden=!d?.supported || state.target>=3;
  $("slider-help").textContent=t(d && !d.supported ? "noAdjustment" : "sliderHelp");
  for(const [id,active] of [["one",!state.all],["all",state.all]]) $(id).setAttribute("aria-pressed",String(active));
}
function drawReadings(d) {
  const list=$("read-list");
  list.replaceChildren();
  for(const [key,field,help,rate] of [["minimum","min","minHelp",engineRate(d)],["default","default","defaultHelp",engineRate(d)],["current","current","currentHelp",d?.current_rate]]) {
    const row=el("div",null,"read-row"), label=el("dt",t(key)), value=el("dd",realMs(period(d?.[field],rate)));
    label.title=t(help);row.title=t(help);
    if(positive(d?.[field])) value.append(el("small",d[field]+" "+t("frames")));
    row.append(label,value);list.append(row);
  }
  const stamp=$("stamp");
  stamp.textContent=state.reading ? t("refreshing") : state.stamp ? (state.readsFailed ? t("lastRead") : t("readAt"))+" "+state.stamp.toLocaleTimeString(state.lang==="en"?"en-GB":"zh-CN",{hour12:false}) : t("wait");
}
function render() {
  document.documentElement.lang=state.lang==="en"?"en":"zh-CN";
  document.documentElement.dataset.theme=state.theme;
  document.title=t("app");
  drawCaption();
  document.querySelectorAll("[data-t]").forEach(n=>{n.textContent=t(n.dataset.t);});
  for(const key of ["zh","en"]) $(key).setAttribute("aria-pressed",String(state.lang===key));
  for(const key of ["dark","light"]) {$(key).setAttribute("aria-pressed",String(state.theme===key));title($(key),t(key));}
  $("workspace").hidden=!state.accepted;
  $("storage-status").textContent=[state.sessionConsent?t("sessionConsent"):"",state.migrationError?t("snapshotStorage")+" "+state.migrationError:""].filter(Boolean).join(" ");
  $("storage-status").hidden=!$("storage-status").textContent;
  $("about").disabled=!state.accepted;
  $("diagnostics").disabled=!state.accepted || !!state.job || !!state.run;
  $("slider").setAttribute("aria-label",t("targetA11y"));
  $("scope").setAttribute("aria-label",t("scope"));
  $("theme").setAttribute("aria-label",t("theme"));
  title($("refresh"),t("refresh"));
  const d=selected(), select=$("output");
  const choices=state.devices.map(item=>[item.id,deviceName(item)+" · "+fmt(item)]);
  if(!d) choices.unshift(["",t(state.devices.length?"pick":"noDevice")]);
  const menuKey=JSON.stringify(choices);
  if(select.dataset.choices!==menuKey) {
    select.replaceChildren(...choices.map(([id,label])=>option(id,label)));
    select.dataset.choices=menuKey;
  }
  if(select.value!==state.id) select.value=state.id;
  select.title=d ? deviceName(d)+" · "+formatDetails(d) : t("pick");
  select.disabled=!!state.job || !!state.run || !!state.reportRequest || state.reading || !state.devices.length;
  const binding=$("binding"), reason=d?.reason || "";
  const uncertain=d?.driver_status==="unknown" || !/不支持|unsupported|other driver/i.test(reason);
  const testOnly=d && !d.supported && testable(d);
  binding.textContent=t(!d?"missing":isRebuildTarget(d)?"supported":testOnly?"testOnly":uncertain?"unknownDriver":"unsupported");
  binding.dataset.ok=String(isRebuildTarget(d));
  const partial=d && (!positive(d.min) || !positive(d.default) || !positive(d.current) || !positive(d.current_rate));
  $("device-note").textContent=(testOnly ? "" : reason) || (partial ? t("partial") : ""); $("device-note").hidden=!$("device-note").textContent;
  for(const key of ["format","mix"]){$(key).textContent=fmt(d,key==="mix");$(key).title=formatDetails(d,key==="mix");}
  $("notice").textContent=state.job ? t("operating")+" · "+t("operatingText") : typeof state.notice==="string" ? state.notice : t(state.notice.key);
  $("notice").hidden=!state.job && !state.notice;
  const permitted=allowed(d) && state.dialog?.kind!=="restart";
  const canApply=permitted && positive(d?.rate) && positive(d?.fundamental) && positive(d?.default) && positive(engineRate(d)) && sliderMax(d)>=.5;
  for(const id of ["apply","slider","one","all"]) $(id).disabled=!canApply;
  $("restore").disabled=!canRebuild(d);
  $("force").disabled=!canRebuild(d);
  $("hard-restart").disabled=!permitted || d?.restart_pending!==true;
  $("hard-restart").title=d?.supported!==true ? t("noSupport") : t("restartHelp");
  $("bench").disabled=!state.run && (!testable(d) || !idle() || state.dialog?.kind==="restart");
  $("refresh").disabled=!!state.run || !!state.job || !!state.reportRequest || state.reading;
  $("restore-help").textContent=!isRebuildTarget(d) ? t("noSupport") : fallbackReason(d) ? t("offerRebuild") : "";
  if(isRebuildTarget(d) && d.snapshot!==true && !state.migrationError)
    $("restore-help").textContent=t("noSnapshot")+" "+t("offerRebuild");
  $("restore").title=$("restore-help").textContent;
  $("bench").textContent=t(state.run?"testing":"test");
  drawTarget(d);drawReadings(d);drawResult(d);
}

function comparison(row, label, type, delta, control, current) {
  const node=el("div",null,"comparison");
  const result=el("strong");
  if(type==="samePeriod") result.textContent=t("same");
  else if(type==="difference" && valid(delta) && valid(control) && valid(current)) {
    result.textContent=Math.abs(delta)<.05 ? t("equal") : t(delta>0?"saved":"increased")+" "+approx(Math.abs(delta));
  } else {result.textContent=t(type==="unmeasured"?"unmeasured":"unavailable");result.className="empty-label";}
  node.append(el("span",label),result);row.append(node);
}
function drawResult(d) {
  const record=state.results.get(d?.id), result=record?.data, bands=result?.bands || {};
  const stale=!!record && (record.invalid || record.condition!==conditions(d));
  const lastOther=!record && state.lastResult && d && state.lastResult.device.id!==d.id;
  const current=result?.currentMs;
  const context=$("test-context");
  $("queue-ms").textContent=valid(current) ? "~"+current.toFixed(1) : "—";
  $("queue-unit").hidden=!valid(current);
  $("queue-ms").parentElement.dataset.empty=String(!valid(current));
  const queueLabel=document.querySelector(".queue-label");
  queueLabel.textContent=stale ? t("previous")+" · "+t("queue") : t(d && !d.supported ? "minimumQueue":"currentSetting");
  if(record) {
    const old=record.device;
    context.textContent=[t("previous"),deviceName(old),fmt(old),t("minimumAtTest")+" "+realMs(period(old.min,engineRate(old)))].join(" · ");
  } else context.textContent=t("noTest");
  const comparisons=$("comparisons");
  comparisons.replaceChildren();
  const def=period(record?.device.default,engineRate(record?.device));
  const defaultLabel=result?.explicitTen===true ? t("vsTen") : t("vsDefault")+(valid(def)?" "+shortMs(def):"");
  comparison(comparisons,defaultLabel,result ? result.defaultComparison : "unmeasured",result?.defaultDelta,result?.defaultMs,current);
  comparison(comparisons,t("vsSeven"),result ? result.sevenComparison : "unmeasured",result?.sevenDelta,result?.sevenMs,current);
  const details=$("test-details"), data=$("test-data");
  details.hidden=!record; data.replaceChildren();
  if(record) {
    for(const [key,titleKey] of [["default",result.explicitTen===true?"ten":"default"],["seven","seven"],["current",currentBand(record.device)]]) {
      const band=bands[key] || {};
      const row=el("div",null,"band");
      const readouts=[t("actual")+" "+realMs(band.periodMs),t("queueMedian")+" "+(valid(band.queueMs)?approx(band.queueMs):t("missing"))];
      row.append(el("span",t(titleKey)),el("strong",readouts.join(" · "))); data.append(row);
      if(Array.isArray(band.rounds)) {
        const validRounds=band.rounds.filter(r=>positive(r?.frames) && valid(r.queueMs)).length;
        data.append(el("p",record.skipped?.has(key) ? t("skipped")+" "+roundsPerBand+" "+t("rounds") : t("validRounds")+" "+validRounds+" / "+roundsPerBand,"meta"));
      }
      if(band.reason) data.append(el("p",band.reason,"meta"));
    }
    if(result.versions) {
      const versions=typeof result.versions==="string" ? result.versions : Object.entries(result.versions).filter(([,v])=>typeof v==="string"||typeof v==="number").map(([k,v])=>k+" "+v).join(" · ");
      if(versions) data.append(el("p",t("library")+" · "+versions,"meta"));
    }
  }
  const status=$("test-state");
  const lines=[];
  if(stale) lines.push(t("stale"));
  if(record) {
    if(result.cancelled) lines.push(t("cancelled"));
    if(result.reason) lines.push(libraryDiagnostic(result.reason));
    lines.push((result.restored===true?"✓ ":"")+t(result.restored===true?"verified":"pending"));
    if(result.restoreReason) lines.push(result.restoreReason);
  } else {
    if(lastOther) lines.push(t("previous")+" · "+deviceName(state.lastResult.device)+" · "+fmt(state.lastResult.device));
    lines.push(t("noTestHelp"));
  }
  status.textContent=lines.join("\n");status.dataset.ok=String(!stale && result?.restored===true && !result?.cancelled && !result?.reason);
}
function closeDialog() {
  state.dialog=null;
  if($("modal").open) $("modal").close();
}
function showDialog(kind, params={}) {
  state.dialog={kind,...params};
  drawDialog();
}
function button(textKey, action, cls, id) {
  const node=el("button",t(textKey),cls);
  if(id) node.id=id;
  node.onclick=action;
  return node;
}
function summaryList(items) {
  const list=el("dl",null,"identity");
  for(const [label,value] of items) list.append(el("dt",t(label)),el("dd",value));
  return list;
}
function actions(...buttons) {const row=el("div",null,"dialog-actions");row.append(...buttons);return row;}
function modelTitle(key){const node=el("h2",t(key));node.id="dialog-title";return node;}
function exit() {send("exit");}
function installCaption() {
  for(const bar of document.querySelectorAll(".titlebar")) {
    bar.append($("window-controls").content.cloneNode(true));
    for(const control of bar.querySelectorAll("[data-window]"))
      control.onclick=()=>send(control.dataset.window);
    // New WebView2 runtimes handle app-region natively. Older runtimes send pointer events here.
    const drag=bar.querySelector(".drag-region");
    drag.onmousedown=e=>{if(e.button===0) send(e.detail===2?"windowMaximize":"windowDrag");};
    drag.ontouchstart=e=>{e.preventDefault();send("windowDrag");};
  }
  for(const parent of [document.body,$("modal")]) {
    for(const edge of ["n","ne","e","se","s","sw","w","nw"]) {
      const handle=el("div",null,"resize-edge");
      handle.dataset.edge=edge;
      handle.setAttribute("aria-hidden","true");
      handle.onpointerdown=e=>{
        if(e.isPrimary && e.button===0 && !state.maximized) {
          e.preventDefault();send("windowResize",{edge});
        }
      };
      parent.append(handle);
    }
  }
}
function drawCaption() {
  document.documentElement.dataset.maximized=String(state.maximized);
  for(const node of document.querySelectorAll("[data-window]")) {
    const key=node.dataset.window;
    title(node,t(key==="exit"?"closeWindow":key==="windowMaximize" && state.maximized?"windowRestore":key));
  }
}
function generateReport() {
  const d=selected();
  if(state.dialog?.kind!=="diagnostics" || !d || state.job || state.run || state.reading || state.reportRequest) return;
  state.reportRequest={id:d.id,requestId:++operationSequence};
  state.reportResult=null;
  send("generateDiagnostic",state.reportRequest);
  render();drawDialog();
}
function finishReport(message,error) {
  const request=state.reportRequest;
  if(!request || message.requestId!==request.requestId || message.id!==request.id) return;
  state.reportRequest=null;
  state.reportResult={id:request.id,error};
  if(typeof message.path==="string") state.diagnosticPath=message.path;
  render();
  if(state.dialog?.kind==="diagnostics") drawDialog();
}
function linkStatus(dialog,root) {
  const status=el("p",dialog.linkError ? t("linkFailed")+" · "+dialog.linkError : dialog.opened?t("linkOpened"):"","modal-status");
  status.setAttribute("role","status");
  root.append(status);
}
function startOperation(dialog) {
  if(state.dialog!==dialog) return;
  const d=selected();
  const permitted=dialog.op==="force" ? canRebuild(d) : allowed(d);
  if(!permitted || confirmationIdentity(dialog.op,d)!==dialog.identity || (dialog.op==="restore" && fallbackReason(d))) {
    closeDialog();state.notice=t("changed");render();return;
  }
  const request=dialog.op==="apply" || dialog.op==="restore" ? {id:d.id,requestId:++operationSequence} : {};
  state.job={device:d,op:dialog.op,...request};state.notice="";
  showDialog("operating");render();
  send(dialog.op,{device:d,ms:dialog.ms,all:dialog.all,...request});
}
function confirmOp(op,reason="") {
  const d=selected();
  if(op==="apply" ? !allowed(d) : !canRebuild(d)) return;
  if(op==="restore") {
    reason=fallbackReason(d);
    if(reason) op="force";
  }
  showDialog("confirm",{op,reason,device:d,identity:confirmationIdentity(op,d),ms:state.target,all:state.all});
}
let operationSequence=0;
function prepareRestart(offered=false) {
  const d=selected();
  if(state.dialog || !allowed(d) || d.restart_pending!==true) return;
  const requestId=++operationSequence, expires=Date.now()+30000;
  showDialog("restart",{device:d,identity:identify(d),requestId,expires,offered});
  const dialog=state.dialog;
  dialog.timer=setTimeout(()=>cancelRestart(dialog,{key:"restartExpired"}),30000);
  render();
  send("prepareRestart",{id:d.id,device:d,requestId});
}
function cancelRestart(dialog,notice) {
  if(state.dialog!==dialog) return;
  clearTimeout(dialog.timer);
  closeDialog();
  send("cancelRestart",{id:dialog.device.id,requestId:dialog.requestId});
  if(notice) state.notice=notice;
  render();
}
function matchesRequest(message,request) {
  return !!request && message.id===request.device.id && message.requestId===request.requestId;
}
function staleOperation(message) {
  if(message.op==="hardRestart" || state.job?.op==="hardRestart")
    return message.op!=="hardRestart" || state.job?.op!=="hardRestart" || !matchesRequest(message,state.job);
  if((message.op==="apply" || message.op==="restore") && message.requestId!==undefined)
    return message.op!==state.job?.op || !matchesRequest(message,state.job);
  return false;
}
function restartValid(dialog) {
  const d=selected();
  if(state.dialog!==dialog) return false;
  if(Date.now()>=dialog.expires) cancelRestart(dialog,{key:"restartExpired"});
  else if(d?.restart_pending!==true || identify(d)!==dialog.identity) cancelRestart(dialog,{key:"changed"});
  return state.dialog===dialog;
}
function restartPrepared(message) {
  const dialog=state.dialog;
  if(dialog?.kind!=="restart" || !matchesRequest(message,dialog) || dialog.token || !restartValid(dialog)) return;
  if(typeof message.token!=="string" || !message.token.trim() || !Number.isSafeInteger(message.port) || message.port<=0) {
    cancelRestart(dialog,{key:"restartInvalid"});return;
  }
  dialog.token=message.token;dialog.port=message.port;
  drawDialog();
}
function startRestart(dialog) {
  if(!restartValid(dialog) || !dialog.token || !allowed(selected())) return;
  clearTimeout(dialog.timer);
  const d=selected();
  state.job={device:d,op:"hardRestart",requestId:dialog.requestId};state.notice="";
  showDialog("operating");render();
  send("hardRestart",{id:d.id,device:d,requestId:dialog.requestId,token:dialog.token});
}
function btnCancel() {return button("cancel",closeDialog,"secondary","safe-action");}
function displayPath(path) {
  if(typeof path!=="string") return "";
  return path.startsWith("\\\\?\\") && /^[A-Za-z]:\\/.test(path.slice(4)) ? path.slice(4) : path;
}
function libraryDiagnostic(message) {
  if(state.lang!=="en" || typeof message!=="string") return message;
  // Keep filenames, missing API names, version numbers and Windows error codes from the host.
  return message
    .replaceAll("Windows 签名验证失败", "Windows could not verify the digital signature")
    .replaceAll("签名发布者不是 Un4seen Developments Ltd", "The signer is not Un4seen Developments Ltd")
    .replaceAll("无法读取已验证的签名发布者", "Windows could not identify the publisher of the verified signature")
    .replace(/(\S+\.dll) 缺少接口 (\w+)/gi, "$1 is missing the required function $2")
    .replace(/(\S+\.dll) 的版本为 ([\d.]+)，试流需要 2\.4 系列接口/g, "$1 reports version $2, which uses an incompatible API")
    .replace(/需要 x64，实际机器类型为 (0x[0-9a-f]+)/gi, "An x64 DLL is required. This file has machine type $1")
    .replaceAll("不是有效的 Windows PE 文件", "Invalid Windows executable format")
    .replaceAll("PE 文件未标记为 DLL", "The Windows executable is not marked as a DLL")
    .replaceAll("Windows 可执行文件头读取失败", "The Windows executable header could not be read")
    .replace(/缺少 (bass(?:wasapi)?\.dll)/gi, "Missing $1")
    .replace(/无法读取 (bass(?:wasapi)?\.dll)/gi, "Cannot read $1")
    .replace(/(bass(?:wasapi)?\.dll) 指向所选目录外部/gi, "$1 resolves outside the selected folder")
    .replace(/(bass(?:wasapi)?\.dll) 加载失败/gi, "Windows could not load $1")
    .replaceAll("所选目录不存在", "The selected folder does not exist")
    .replaceAll("所选路径不是目录", "The selected path is not a folder")
    .replaceAll("无权读取所选目录", "Permission denied when reading the selected folder")
    .replaceAll("无法读取所选目录", "Cannot read the selected folder")
    .replaceAll("无法读取 current 目录", "Cannot read the current subfolder")
    .replaceAll("无法打开目录", "Cannot open the folder")
    .replaceAll("无法列出所选目录，检查读取权限", "Cannot list the selected folder. Check read permissions")
    .replace(/请选择本机磁盘上的(?:绝对路径|游戏目录)|游戏目录必须位于可读取的本机磁盘上/g, "Select a readable folder on a local drive")
    .replaceAll("；","\n").replaceAll("："," — ").replaceAll("（"," (").replaceAll("）",")").replaceAll("。",".");
}
function prepareRun(message="",retry=false) {
  const d=selected();
  if(!d) return;
  state.pending={id:d.id,identity:identify(d),verified:false};
  state.checkingDir=!retry;state.choosingDir=false;
  showDialog("prepare",{message,retry});
  if(!retry) send("checkDir");
}
function startRun() {
  if(state.run || !state.pending?.verified || state.checkingDir || state.choosingDir) return;
  const d=selected();
  if(!idle() || !testable(d) || !state.pending || state.pending.identity!==identify(d)) {
    state.pending=null;closeDialog();state.notice=t("changed");render();return;
  }
  state.pending=null;
  const requestId=++operationSequence;
  state.run={device:{...d},condition:conditions(d),requestId,done:0,total:testBands.length*roundsPerBand,stage:"prepare",cancelled:false};
  state.notice="";
  showDialog("progress");render();
  send("bench",{device:d,requestId});
}
function requestRun() {
  if(state.run) {showDialog("progress");return;}
  const d=selected(); if(!idle() || !testable(d) || state.dialog) return;
  prepareRun();
}
function cancelTest() {
  if(!state.run || state.run.cancelled) return;
  state.run.cancelled=true;send("cancelBench");drawDialog();
}
function drawProgress(root) {
  const run=state.run;
  if(!run) {closeDialog();return;}
  root.append(modelTitle("progress"),el("p",deviceName(run.device)+" · "+fmt(run.device)),el("p",t("queueHelp")));
  const phase=({prepare:t("prepare"),default:t("measuring")+" "+t("default"),seven:t("measuring")+" "+t("seven"),current:t("measuring")+" "+t(currentBand(run.device)),verify:t("verify"),complete:t("complete")})[run.stage] || run.stage;
  const progress=el("progress"), head=el("div",null,"progress-head");
  const ended=run.done+(run.skippedCount || 0);
  const count=t("finishedRounds")+" "+run.done+" / "+run.total+" "+t("rounds")+(run.skippedCount ? " · "+t("skipped")+" "+run.skippedCount+" "+t("rounds") : "");
  head.append(el("strong",phase),el("span",count));
  progress.max=run.total;progress.value=ended;progress.setAttribute("aria-label",phase);progress.setAttribute("aria-valuetext",count);
  root.append(head,progress);
  const rows=el("div",null,"progress-bands");
  for(const [i,key] of testBands.entries()) {
    const n=Math.min(roundsPerBand,Math.max(0,ended-i*roundsPerBand)), row=el("div",null,"band");
    row.dataset.done=String(n===roundsPerBand && !run.skipped?.has(key));
    row.append(el("span",t(key==="current"?currentBand(run.device):key)),el("strong",run.skipped?.has(key)?t("skipped"):n+" / "+roundsPerBand));rows.append(row);
  }
  const cancel=button(run.cancelled?"cancelling":"cancelBench",cancelTest);
  cancel.disabled=run.cancelled;
  root.append(rows,el("p",t("verifyingHelp")),actions(cancel,button("hide",closeDialog,"secondary","safe-action")));
}
function drawDialog() {
  const dialog=state.dialog;if(!dialog) return;
  const modal=$("modal"), root=$("dialog-content");
  const wasOpen=modal.open, keyWas=document.activeElement?.id;
  root.replaceChildren();
  if(dialog.kind==="consent") {
    const langs=el("div",null,"lang-dialog"), b=el("button",state.lang==="zh"?"English":"中文");
    b.onclick=()=>setLanguage(state.lang==="zh"?"en":"zh");langs.append(b);
    root.append(langs,modelTitle("safety"),el("p",t("safetyText")));
    if(state.preferencesError) root.append(el("p",t("preferenceStorage")+" "+state.preferencesError,"risk"));
    if(state.consentError) root.append(el("p",state.consentError,"risk"));
    const row=actions(button("no",exit,"secondary","safe-action"),button("yes",()=>send("accept"),"primary"));
    if(state.preferencesError || state.consentError) row.append(button("sessionAccept",()=>send("acceptSession"),"secondary"));
    root.append(row);
  } else if(dialog.kind==="about") {
    const about=el("section",null,"about-page");
    const heading=el("h2","UAC2 Control");
    heading.id="dialog-title";
    if(state.version) heading.append(" ",el("span",state.version,"about-version"));
    const close=button("close",closeDialog,"about-close","safe-action");
    title(close,t("close"));
    const head=el("div",null,"about-heading");
    head.append(heading,close);
    const repo=button("repository",()=>send("openRepository"),"repository-button","repository-link");
    repo.replaceChildren($("github-mark").content.cloneNode(true));
    title(repo,t("repository"));
    about.append(head,el("p",t("aboutPurpose"),"about-purpose"),repo);
    linkStatus(dialog,about);
    about.append(el("p",t("aboutLicense"),"about-license"));
    root.append(about);
  } else if(dialog.kind==="diagnostics") {
    const d=selected(), result=state.reportResult;
    const generate=button("generate",generateReport,"primary","generate-report");
    generate.disabled=!d || !!state.reportRequest || state.reading || !!state.job || !!state.run;
    const message=state.reportRequest ? t("generating") : !d ? t("noTarget") :
      result?.id===d.id ? (result.error?t("reportFailed")+" · "+result.error:t("generated")) : "";
    const status=el("p",message,"modal-status");
    status.setAttribute("role","status");
    const target=summaryList([["output",deviceName(d)]]);
    if(d) {
      const name=target.querySelector("dd");
      name.id="diagnostic-target-"+d.id;
      name.tabIndex=0;
    }
    root.append(modelTitle("diagnostics"),el("p",t("probeHelp")),target,
      el("p",t("reportLocation")),el("pre",displayPath(state.diagnosticPath) || "uac2-data/diagnostics/latest.json","report-path"),status,
      actions(button("close",closeDialog,"secondary","safe-action"),button("openIssue",()=>send("openIssue"),"secondary","issue-link"),generate));
    linkStatus(dialog,root);
  } else if(dialog.kind==="confirm") {
    const d=dialog.device, op=dialog.op, titleKey={apply:"confirmApply",restore:"confirmRestore",force:"confirmForce"}[op];
    const list=[["output",deviceName(d)],["usb",d.function_id || t("missing")]];
    const names=Array.isArray(d.siblings)?d.siblings.filter(n=>typeof n==="string" && n.trim()):[];
    list.push(["affected",names.length ? Array.from(new Set([deviceName(d),...names])).join(" · ") : deviceName(d)+" · "+t("unknownEndpoints")]);
    if(op==="apply") list.push(["targetScope",realMs(dialog.ms)+" · "+t(dialog.all?"all":"one")+(dialog.all?"":" · "+rateText(d.rate))]);
    root.append(modelTitle(titleKey));
    if(dialog.reason) root.append(el("p",dialog.reason,"risk"));
    root.append(summaryList(list),el("p",t(op+"Text")));
    if(op==="apply" && dialog.ms<3) root.append(el("p",t("low"),"risk"));
    root.append(actions(btnCancel(),button(op==="force"?"confirmForce":op,()=>startOperation(dialog),op==="force"?"danger":"primary")));
  } else if(dialog.kind==="restart") {
    const d=dialog.device, list=[["output",deviceName(d)],["usb",d.function_id || t("missing")]];
    const cancel=button("cancel",()=>cancelRestart(dialog),"secondary","safe-action");
    root.append(modelTitle("confirmRestart"));
    if(dialog.offered) root.append(el("p",t("restartSuggested")));
    if(dialog.token) {
      list.push(["usbPort",String(dialog.port)]);
      const confirm=button("hardRestart",()=>startRestart(dialog),"danger");
      confirm.disabled=!allowed(selected());
      root.append(summaryList(list),el("p",t("restartText")),actions(cancel,confirm));
    } else root.append(summaryList(list),el("p",t("restartChecking")),actions(cancel));
  } else if(dialog.kind==="operating") {
    root.append(modelTitle("operating"),el("p",t("operatingText")),actions(button("hide",closeDialog,"secondary","safe-action")));
  } else if(dialog.kind==="prepare") {
    const d=selected();
    const cancel=button("cancel",()=>{state.pending=null;closeDialog();},"secondary","safe-action");
    const choose=button(dialog.retry || state.dirPath || state.hasDir ? "rechoose":"choose",()=>{
      if(!state.pending || state.choosingDir || state.checkingDir) return;
      state.choosingDir=true;drawDialog();send("chooseDir");
    },"secondary","choose-dir");
    const start=button("start",()=>{if(state.dialog===dialog) startRun();},"primary","start-test");
    choose.disabled=state.checkingDir || state.choosingDir;
    start.disabled=!state.pending?.verified || state.checkingDir || state.choosingDir || !idle() || !testable(d);
    const path=displayPath(dialog.attemptedPath || state.dirPath) || t(state.hasDir ? "pathMissing" : "noDir");
    root.append(modelTitle("dirTitle"),summaryList([["output",deviceName(d)],d?.supported?["format",fmt(d)]:["mix",fmt(d,true)],["minimum",realMs(period(d?.min,engineRate(d)))],["dirLabel",path]]),el("p",t("testPurpose")),el("p",t("dirText")),el("p",t(d?.supported?"prepareHelp":"prepareReadOnly")));
    if(state.checkingDir) root.append(el("p",t("dirChecking")));
    else if(dialog.message) root.append(el("p",t("libraryError")+" · "+libraryDiagnostic(dialog.message),"risk"));
    else if(state.pending?.verified) root.append(el("p","✓ "+t("dirReady"),"ready-state"));
    root.append(actions(cancel,choose,start));
  } else if(dialog.kind==="progress") drawProgress(root);
  if(!wasOpen) {modal.showModal();$("safe-action")?.focus();}
  else if(keyWas) $(keyWas)?.focus({preventScroll:true});
}

function receiveDevices(message) {
  const automatic=message.automatic===true;
  if(automatic && state.reading) return;
  const before=selected(), old=conditions(before);
  state.devices=Array.isArray(message.devices) ? message.devices : [];
  const returning=message.selected===state.offlineId ? state.offlineId : "";
  const desired=automatic ? state.id || returning : message.selected ?? state.id;
  state.id=state.devices.some(d=>d.id===desired) ? desired : "";
  const d=selected();
  if(before && !d) state.offlineId=before.id;
  else if(d) state.offlineId="";
  const changed=old!==conditions(d);
  const formatChanged=formatKey(before)!==formatKey(d);
  for(const [id,record] of state.results) {
    const fresh=state.devices.find(item=>item.id===id);
    if(conditions(fresh)!==record.condition) record.invalid=true;
  }
  if(state.run && conditions(state.devices.find(item=>item.id===state.run.device.id))!==state.run.condition) state.run.invalid=true;
  state.reading=false;state.readsFailed=false;state.stamp=d ? new Date() : null;
  if(formatChanged || !state.dirty) draftFor(d);
  else if(d) state.target=Math.min(state.target,Math.max(.5,sliderMax(d)));
  if(before && !d) state.notice={key:"offline"};
  else if(automatic && formatChanged && d) state.notice={key:"formatChanged"};
  if(state.dialog?.kind==="restart") restartValid(state.dialog);
  if(state.dialog?.kind==="confirm" && state.dialog.identity!==confirmationIdentity(state.dialog.op,d) || state.pending && state.pending.identity!==identify(d)) {
    state.pending=null;closeDialog();state.notice={key:"changed"};
  }
  if(changed) $("test-details").open=false;
  render();
  if(state.dialog?.kind==="diagnostics" || !automatic && ["prepare","restart"].includes(state.dialog?.kind)) drawDialog();
  if(!automatic && state.restartOffer!==null) {
    const original=state.restartOffer;
    state.restartOffer=null;
    if(d?.restart_pending===true && original===formatKey(d)) {
      state.rebuildOffer=null;prepareRestart(true);
    }
  }
  if(!automatic && state.rebuildOffer) {
    const offer=state.rebuildOffer;state.rebuildOffer=null;
    if(!state.dialog && canRebuild(d) && offer.key===rebuildKey(d))
      confirmOp("force",t("failedSnapshot")+" "+offer.message);
  }
}
function libraryIssue(message) {return /\.dll|x64|64.bit|64 位|目录|文件夹|directory|folder|library|libraries|音频库/i.test(message);}
function matchesRun(message,run) {
  return !!run && (!message.deviceId || message.deviceId===run.device.id) &&
    (message.requestId===undefined || message.requestId===run.requestId);
}
function receiveResult(message) {
  const run=state.run;
  if(!matchesRun(message,run)) return;
  const record={device:run.device,condition:run.condition,data:message.data || {},invalid:!!run.invalid,skipped:run.skipped};
  state.results.set(run.device.id,record);state.lastResult=record;
  state.run=null;if(state.dialog?.kind==="progress") closeDialog();
  if(libraryIssue(record.data.reason || "")) {
    prepareRun(record.data.reason,true);
  }
  refresh();render();
}
function receiveError(message) {
  if(message.automatic===true) return;
  const error=message.message || t("opError");
  if(message.op==="generateDiagnostic") {finishReport(message,error);return;}
  if(message.op==="openIssue" || message.op==="openRepository") {
    const kind=message.op==="openIssue"?"diagnostics":"about";
    if(state.dialog?.kind===kind) {state.dialog.linkError=error;drawDialog();}
    return;
  }
  if(message.op==="bench" && !matchesRun(message,state.run)) return;
  if(message.op==="cancelRestart") return;
  if(message.op==="prepareRestart") {
    const dialog=state.dialog;
    if(dialog?.kind==="restart" && !dialog.token && matchesRequest(message,dialog)) cancelRestart(dialog,error);
    return;
  }
  if(staleOperation(message)) return;
  if(!state.accepted) {
    if(state.dialog?.kind==="consent") {
      if(message.op==="accept") {state.consentError=error;drawDialog();}
      else $("dialog-content").append(el("p",error,"risk"));
    }
    return;
  }
  if(state.run && (!message.op || message.op==="bench" || message.op==="cancelBench")) {
    receiveResult({deviceId:state.run.device.id,data:{restored:false,reason:error}});
    return;
  }
  if(message.op==="chooseDir" || message.op==="checkDir") {
    state.choosingDir=false;state.checkingDir=false;
    if(state.pending && state.dialog?.kind==="prepare") {
      state.pending.verified=false;showDialog("prepare",{message:error,retry:true});
    }
    return;
  }
  const hadJob=!!state.job;
  if(hadJob) {
    rememberFailedRestore(message);
    if(state.dialog?.kind==="operating") closeDialog();
    const old=state.results.get(state.job.device.id);if(old) old.invalid=true;
    state.job=null;
  }
  if(state.reading || message.op==="refresh" || message.op==="list") {
    state.readsFailed=true;state.reading=false;
    state.restartOffer=null;
    state.rebuildOffer=null;
    if(state.dialog?.kind==="restart") cancelRestart(state.dialog);
    state.notice=t("retryRead")+" "+error;
  } else state.notice=error;
  render();
  if(hadJob && !state.reading) refresh();
}
function rememberFailedRestore(message) {
  if(message.op==="restore" && state.job?.op==="restore" && matchesRequest(message,state.job))
    state.rebuildOffer={key:rebuildKey(state.job.device),message:message.message || ""};
}
window.onNative=message=>{
  if(!message || typeof message!=="object") return;
  switch(message.type) {
    case "init":
      state.initialized=true;
      state.theme=message.theme==="light"?"light":"dark";
      state.lang=message.lang==="en"?"en":"zh";
      state.version=message.version || "";
      state.diagnosticPath=message.diagnosticPath || "";
      state.migrationError=message.migrationError || "";
      state.preferencesError=message.preferencesError || "";
      state.sessionConsent=message.sessionConsent===true;
      state.consentError="";
      state.hasDir=message.hasDir===true;state.dirPath=typeof message.path==="string"?message.path:"";state.accepted=message.accepted===true;
      render();
      if(!state.accepted) showDialog("consent");
      else {if(state.dialog?.kind==="consent") closeDialog();refresh();}
      break;
    case "devices": if(state.accepted) receiveDevices(message);break;
    case "windowState": state.maximized=message.maximized===true;drawCaption();break;
    case "diagnostic": finishReport(message,"");break;
    case "linkOpened":
      if(state.dialog?.kind===(message.op==="openIssue"?"diagnostics":"about")) {
        state.dialog.opened=true;state.dialog.linkError="";drawDialog();
      }
      break;
    case "restartPrepared": restartPrepared(message);break;
    case "error": receiveError(message);break;
    case "directory": {
      state.checkingDir=false;state.choosingDir=false;
      if(message.ok || message.checked) {
        if(typeof message.path==="string") state.dirPath=message.path;
        else if(message.checked && message.path===null) state.dirPath="";
        state.hasDir=!!state.dirPath || message.ok===true;
      }
      if(state.pending && state.dialog?.kind==="prepare") {
        if(!message.cancelled) {
          state.pending.verified=message.ok===true;
          state.dialog.message=message.message || "";
          state.dialog.retry=!message.ok && !!message.message;
          state.dialog.attemptedPath=!message.ok && typeof message.path==="string" ? message.path : "";
        }
        drawDialog();
      }
      render();break;
    }
    case "needDirectory": {
      if(!matchesRun(message,state.run)) break;
      state.run=null;
      prepareRun(message.message || "",true);render();break;
    }
    case "progress": {
      const run=state.run, p=message.data || message;
      if(!matchesRun(message,run) || (p.total!==undefined && p.total!==run.total) || (valid(p.done) && p.done<run.done)) break;
      if(valid(p.done)) run.done=Math.min(run.total,Math.max(0,p.done));
      if(typeof p.stage==="string") run.stage=p.stage;
      if(valid(p.skipped) && p.skipped>(run.skippedCount || 0)) {
        run.skippedCount=p.skipped;run.skipped??=new Set();run.skipped.add(run.stage);
      }
      if(state.dialog?.kind==="progress") drawDialog();
      break;
    }
    case "result": receiveResult(message);break;
    case "operation":
      if(staleOperation(message)) break;
      if(message.ok===false) rememberFailedRestore(message);
      if(message.offerRestart===true && (message.op==="apply" || message.op==="restore") &&
          message.op===state.job?.op && matchesRequest(message,state.job))
        state.restartOffer=formatKey(state.job.device);
      if(state.job) {const record=state.results.get(state.job.device.id);if(record) record.invalid=true;}
      state.job=null;state.notice=message.message || t(message.ok?"done":"opError");
      if(state.dialog?.kind==="operating") closeDialog();
      state.dirty=false;refresh();render();break;
  }
};
installCaption();
$("diagnostics").onclick=()=>showDialog("diagnostics");
$("about").onclick=()=>showDialog("about");
for(const lang of ["zh","en"]) $(lang).onclick=()=>setLanguage(lang);
for(const theme of ["dark","light"]) $(theme).onclick=()=>{state.theme=theme;setPrefs();render();};
$("output").onchange=()=>{
  state.restartOffer=null;
  state.rebuildOffer=null;
  if(state.dialog?.kind==="restart") cancelRestart(state.dialog);
  const d=state.devices.find(x=>x.id===$("output").value);
  state.id=d?.id || "";state.offlineId="";draftFor(d);state.notice="";state.readsFailed=false;
  state.pending=null;$("test-details").open=false;refresh();
};
$("slider").oninput=e=>{state.target=Number(e.target.value);state.dirty=true;drawTarget(selected());};
for(const key of ["one","all"]) $(key).onclick=()=>{state.all=key==="all";state.dirty=true;drawTarget(selected());};
for(const key of ["apply","restore","force"]) $(key).onclick=()=>confirmOp(key);
$("hard-restart").onclick=()=>prepareRestart();
$("refresh").onclick=()=>{state.notice="";refresh();};
$("bench").onclick=requestRun;
$("modal").addEventListener("cancel",e=>{
  e.preventDefault();
  if(state.dialog?.kind==="consent") {exit();return;}
  if(state.dialog?.kind==="restart") {cancelRestart(state.dialog);return;}
  if(state.dialog?.kind==="prepare") state.pending=null;
  closeDialog();
});
let deviceChangeTimer;
function scheduleDeviceRead() {
  clearTimeout(deviceChangeTimer);
  deviceChangeTimer=setTimeout(()=>{
    if(state.accepted && !state.run && !state.job && !state.reportRequest && !state.choosingDir && document.visibilityState==="visible") refresh();
  },250);
}
navigator.mediaDevices?.addEventListener("devicechange",scheduleDeviceRead);
document.addEventListener("visibilitychange",()=>{if(document.visibilityState==="visible") scheduleDeviceRead();});
render();
send("ready");
