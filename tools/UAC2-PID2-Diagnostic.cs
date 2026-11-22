using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Security.AccessControl;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;
using System.Windows.Forms;
using Microsoft.Win32;
using Row = System.Collections.Generic.Dictionary<string, object>;

// x64 .NET Framework. MMDevice declarations follow tools/uac2/EndpointReadProbe.cs.
// The only COM property access is STGM_READ. No audio client, stream, or writer is opened.
internal static class Pid2Diagnostic
{
    const string Pid = "{a2a3fff4-353f-407c-9d86-1f9dc7d5a606},2";
    const string RenderPrefix = "{0.0.0.00000000}.";
    const string DriverGuid = "a8b865dd-2e3d-4094-ad97-e593a70c75d6";
    static readonly byte[] LegacyPrefix = {65, 0, 0, 0, 1, 0, 0, 0};

    [StructLayout(LayoutKind.Sequential)]
    struct Key {
        public Guid Guid;
        public uint Id;
        public Key(string guid, uint id) { Guid = new Guid(guid); Id = id; }
    }
    [StructLayout(LayoutKind.Explicit, Size=24)]
    struct Variant {
        [FieldOffset(0)] public ushort Type;
        [FieldOffset(8)] public uint Count;
        [FieldOffset(8)] public IntPtr Pointer;
        [FieldOffset(16)] public IntPtr Blob;
    }
    [ComImport, Guid("A95664D2-9614-4F35-A746-DE8DB63617E6"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface Enumerator {
        [PreserveSig] int EnumAudioEndpoints(int flow, uint state, out Collection result);
        [PreserveSig] int GetDefaultAudioEndpoint(int flow, int role, out Endpoint ep);
        [PreserveSig] int GetDevice([MarshalAs(UnmanagedType.LPWStr)] string id, out Endpoint ep);
    }
    [ComImport, Guid("0BD7A1BE-7A1A-44DB-8397-CC5392387B5E"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface Collection {
        [PreserveSig] int GetCount(out uint count);
        [PreserveSig] int Item(uint index, out Endpoint ep);
    }
    [ComImport, Guid("D666063F-1587-4E43-81F1-B948E807363F"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface Endpoint {
        [PreserveSig] int Activate(ref Guid iid, uint ctx, IntPtr param, out IntPtr result);
        [PreserveSig] int OpenPropertyStore(uint mode, out Store store);
        [PreserveSig] int GetId([MarshalAs(UnmanagedType.LPWStr)] out string id);
        [PreserveSig] int GetState(out uint state);
    }
    [ComImport, Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    interface Store {
        [PreserveSig] int GetCount(out uint count);
        [PreserveSig] int GetAt(uint index, out Key key);
        [PreserveSig] int GetValue(ref Key key, out Variant v);
    }
    [DllImport("ole32.dll")] static extern int PropVariantClear(ref Variant v);
    [DllImport("cfgmgr32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
    static extern uint CM_Locate_DevNodeW(out uint node, string id, uint flags);
    [DllImport("cfgmgr32.dll")] static extern uint CM_Get_Parent(out uint parent, uint node, uint flags);
    [DllImport("cfgmgr32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
    static extern uint CM_Get_DevNode_PropertyW(uint node, ref Key key, out uint type, [Out] byte[] data, ref uint size, uint flags);
    [DllImport("cfgmgr32.dll", CharSet=CharSet.Unicode, ExactSpelling=true)]
    static extern uint CM_Get_Device_IDW(uint node, StringBuilder id, uint length, uint flags);

    static void Check(int hr) { Marshal.ThrowExceptionForHR(hr); }
    static void Release(object o) { if (o != null) Marshal.ReleaseComObject(o); }
    static string Error(Exception ex) { return ex.GetType().Name + " 0x" + ex.HResult.ToString("X8"); }
    static string Now() { return DateTime.UtcNow.ToString("o"); }
    static string Property(uint node, string guid, uint pid) {
        Key k = new Key(guid, pid);
        byte[] data = new byte[2048];
        uint size = (uint)data.Length, type;
        if (CM_Get_DevNode_PropertyW(node, ref k, out type, data, ref size, 0) != 0 ||
            type != 0x12 || size < 2 || size > data.Length || size % 2 != 0) return null;
        return Encoding.Unicode.GetString(data, 0, (int)size).TrimEnd('\0');
    }
    static Row Identify(string id, ref int unidentified) {
        uint node, function;
        if (CM_Locate_DevNodeW(out node, @"SWD\MMDEVAPI\" + id, 0) != 0 ||
            CM_Get_Parent(out function, node, 0) != 0) { unidentified++; return null; }
        var instance = new StringBuilder(512);
        if (CM_Get_Device_IDW(function, instance, 512, 0) != 0) { unidentified++; return null; }
        if (!instance.ToString().StartsWith(@"USB\VID_", StringComparison.OrdinalIgnoreCase)) return null;
        string service = Property(function, "a45c254e-df1c-4efd-8020-67d146a850e0", 6);
        string provider = Property(function, DriverGuid, 9), inf = Property(function, DriverGuid, 5);
        if (service == null || provider == null || inf == null) { unidentified++; return null; }
        if (!String.Equals(service, "usbaudio2", StringComparison.OrdinalIgnoreCase) ||
            !String.Equals(provider, "Microsoft", StringComparison.OrdinalIgnoreCase) ||
            !String.Equals(inf, "usbaudio2.inf", StringComparison.OrdinalIgnoreCase)) return null;
        return new Row {{"service",service},{"provider",provider},{"inf",inf},{"version",Property(function, DriverGuid, 3)}};
    }
    static RegistryKey ReadKey(RegistryKey root, string path) {
        return root.OpenSubKey(path, RegistryKeyPermissionCheck.Default, RegistryRights.QueryValues);
    }
    static void SaveBytes(Row row, byte[] data) {
        row["length"] = data.Length;
        row["base64"] = Convert.ToBase64String(data);
    }
    static byte[] Blob(Store s, Key key, Row row, out ushort? variantType) {
        variantType = null;
        Variant v;
        int hr = s.GetValue(ref key, out v);
        try {
            row["hresult"] = "0x" + hr.ToString("X8");
            row["variantType"] = v.Type;
            if (hr < 0) return null;
            variantType = v.Type;
            if (v.Type != 65 && v.Type != 0x1011) return null;
            if (v.Count > 4 * 1024 * 1024 || (v.Count != 0 && v.Blob == IntPtr.Zero)) {
                row["error"] = "Invalid blob length or pointer"; return null;
            }
            var data = new byte[v.Count];
            if (data.Length != 0) Marshal.Copy(v.Blob, data, 0, data.Length);
            return data;
        } finally { PropVariantClear(ref v); }
    }
    static void Metadata(Store s, Row endpoint) {
        Key key = new Key("a45c254e-df1c-4efd-8020-67d146a850e0", 14);
        Variant v;
        int hr = s.GetValue(ref key, out v);
        try { if (hr >= 0 && v.Type == 31) endpoint["name"] = Marshal.PtrToStringUni(v.Pointer); }
        finally { PropVariantClear(ref v); }
        var fmt = new Row();
        ushort? formatType;
        byte[] b = Blob(s, new Key("f19f064d-082c-4e27-bc73-6882a1bb8e4c", 0), fmt, out formatType);
        if (b != null && b.Length >= 16) {
            fmt["channels"] = BitConverter.ToUInt16(b, 2);
            fmt["sampleRate"] = BitConverter.ToUInt32(b, 4);
            fmt["bitsPerSample"] = BitConverter.ToUInt16(b, 14);
            if (BitConverter.ToUInt16(b, 0) == 65534 && b.Length >= 40 && BitConverter.ToUInt16(b,16) >= 22)
                fmt["validBitsPerSample"] = BitConverter.ToUInt16(b, 18);
        }
        endpoint["deviceFormat"] = fmt;
    }
    static int? FirstDifference(byte[] disk, byte[] payload, bool legacyExact) {
        int expectedLength = LegacyPrefix.Length + payload.Length;
        int commonLength = Math.Min(disk.Length, expectedLength);
        for (int i = 0; i < commonLength; i++) {
            if (!legacyExact && (i == 2 || i == 3)) continue;
            byte expected = i < LegacyPrefix.Length ? LegacyPrefix[i] : payload[i - LegacyPrefix.Length];
            if (disk[i] != expected) return i;
        }
        if (disk.Length != expectedLength) return commonLength;
        return null;
    }
    static void Compare(Row sample, byte[] payload, ushort? variantType, byte[] disk, RegistryValueKind? kind) {
        sample["matchesApplicationWrapper"] = null;
        sample["firstDifferentOffset"] = null;
        if (payload != null && disk != null) {
            int? legacyOffset = FirstDifference(disk, payload, true);
            sample["matchesLegacyExactWrapper"] = !legacyOffset.HasValue;
            sample["firstLegacyDifferentOffset"] = legacyOffset;
            sample["matchesPayloadWithoutWrapper"] = disk.SequenceEqual(payload);
        }
        string reason = null;
        if (variantType.HasValue && variantType != 65) reason = "propertyType";
        else if (kind.HasValue && kind != RegistryValueKind.Binary) reason = "registryType";
        else if (payload != null && (payload.Length == 0 || payload.Length > 1024 * 1024)) reason = "propertyLength";
        else if (payload != null && disk != null) {
            int? offset = FirstDifference(disk, payload, false);
            sample["matchesApplicationWrapper"] = !offset.HasValue;
            sample["firstDifferentOffset"] = offset;
            return;
        }
        if (reason != null) {
            sample["matchesApplicationWrapper"] = false;
            sample["wrapperMismatchReason"] = reason;
        }
    }
    static Row Sample(Enumerator e, RegistryKey root, string id, string guid, Row endpoint) {
        var p = new Row(); var r = new Row();
        var sample = new Row {{"utc",Now()},{"propertyStore",p},{"registry",r}};
        Endpoint ep = null; Store store = null;
        byte[] payload = null, disk = null;
        ushort? variantType = null;
        RegistryValueKind? kind = null;
        try {
            Check(e.GetDevice(id, out ep));
            string actual; uint state;
            Check(ep.GetId(out actual)); Check(ep.GetState(out state));
            if (!String.Equals(id, actual, StringComparison.OrdinalIgnoreCase) || state != 1)
                throw new InvalidOperationException();
            Check(ep.OpenPropertyStore(0, out store));
            payload = Blob(store, new Key("a2a3fff4-353f-407c-9d86-1f9dc7d5a606", 2), p, out variantType);
            if (payload != null) SaveBytes(p, payload);
            if (!endpoint.ContainsKey("deviceFormat")) {
                try { Metadata(store, endpoint); } catch (Exception ex) { endpoint["metadataError"] = Error(ex); }
            }
        } catch (Exception ex) { p["error"] = Error(ex); }
        finally { Release(store); Release(ep); }
        string path = @"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\" + guid + @"\Properties";
        try {
            using (var key = ReadKey(root, path)) {
                if (key == null) r["status"] = "keyMissing";
                else if (!key.GetValueNames().Contains(Pid, StringComparer.OrdinalIgnoreCase)) r["status"] = "valueMissing";
                else {
                    kind = key.GetValueKind(Pid);
                    r["kind"] = kind.ToString();
                    if (kind == RegistryValueKind.Binary) {
                        disk = key.GetValue(Pid) as byte[];
                        if (disk != null) SaveBytes(r, disk); else r["error"] = "Value changed during read";
                    }
                }
            }
        } catch (Exception ex) { r["error"] = Error(ex); }
        Compare(sample, payload, variantType, disk, kind);
        return sample;
    }
    static Row Collect() {
        var endpoints = new List<Row>();
        var report = new Row {{"schema",2},{"startedUtc",Now()},{"propertyKey",Pid},
            {"registryLocation",@"HKLM64\SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\{endpoint-guid}\Properties"},
            {"expectedPrefixHex","4100000001000000"},
            {"wrapperRule","VT_BLOB (1..1048576 bytes); REG_BINARY = 41 00 ?? ?? 01 00 00 00 + entire unchanged property payload"},
            {"comparisonFields","matchesApplicationWrapper and firstDifferentOffset use wrapperRule. matchesLegacyExactWrapper and firstLegacyDifferentOffset reproduce the schema 1 byte comparison using expectedPrefixHex. Offsets are zero-based registry byte positions, including the first missing or extra byte."},
            {"endpoints",endpoints}};
        int unidentified = 0;
        Enumerator enumerator = null; Collection collection = null;
        try {
            using (var root = RegistryKey.OpenBaseKey(RegistryHive.LocalMachine, RegistryView.Registry64)) {
                using (var os = ReadKey(root, @"SOFTWARE\Microsoft\Windows NT\CurrentVersion")) {
                    if (os != null) report["windows"] = new Row {{"build",os.GetValue("CurrentBuildNumber")},
                        {"ubr",os.GetValue("UBR")},{"displayVersion",os.GetValue("DisplayVersion")}};
                }
                enumerator = (Enumerator)Activator.CreateInstance(Type.GetTypeFromCLSID(new Guid("BCDE0395-E52F-467C-8E3D-C4579291692E")));
                Check(enumerator.EnumAudioEndpoints(0, 1, out collection));
                uint count; Check(collection.GetCount(out count));
                for (uint i = 0; i < count; i++) {
                    Endpoint item = null; Row row = null;
                    try {
                        Check(collection.Item(i, out item));
                        string id; Check(item.GetId(out id)); Guid g;
                        if (!id.StartsWith(RenderPrefix, StringComparison.OrdinalIgnoreCase) ||
                            !Guid.TryParseExact(id.Substring(RenderPrefix.Length), "B", out g)) { unidentified++; continue; }
                        Row driver = Identify(id, ref unidentified);
                        if (driver == null) continue;
                        string guid = g.ToString("B");
                        var samples = new List<Row>();
                        row = new Row {{"endpointId",id},{"driver",driver},{"samples",samples}};
                        endpoints.Add(row);
                        try {
                            using (var snapshots = ReadKey(root, @"SOFTWARE\UAC2 Shared Period\EndpointProfiles\v1")) {
                                bool exists = snapshots != null && snapshots.GetValueNames().Contains(guid, StringComparer.OrdinalIgnoreCase);
                                row["snapshotValueExists"] = exists;
                                if (exists) {
                                    byte[] bytes = snapshots.GetValue(guid) as byte[];
                                    object pending;
                                    if (bytes != null && bytes.Length <= 4 * 1024 * 1024 &&
                                        new JavaScriptSerializer { MaxJsonLength = 4 * 1024 * 1024 }.Deserialize<Row>(
                                            Encoding.UTF8.GetString(bytes)).TryGetValue("reload_pending", out pending) && pending is bool)
                                        row["snapshotReloadPending"] = pending;
                                    else row["snapshotCheckError"] = "Snapshot type, size or reload_pending is unrecognized";
                                }
                            }
                        } catch (Exception ex) { row["snapshotCheckError"] = Error(ex); }
                        samples.Add(Sample(enumerator, root, id, guid, row));
                        Thread.Sleep(250);
                        samples.Add(Sample(enumerator, root, id, guid, row));
                    } catch (Exception ex) {
                        if (row == null) unidentified++; else row["error"] = Error(ex);
                    } finally { Release(item); }
                }
            }
        } catch (Exception ex) { report["collectionError"] = Error(ex); }
        finally { Release(collection); Release(enumerator); }
        report["unidentifiedCount"] = unidentified;
        report["finishedUtc"] = Now();
        return report;
    }
    [STAThread]
    static int Main(string[] args) {
        bool quiet = args.Length == 1 && args[0] == "--quiet";
        if (args.Length != 0 && !quiet) return 2;
        string filename = "UAC2-PID2-" + DateTime.Now.ToString("yyyyMMdd-HHmmss-fff") + ".json";
        try {
            var report = Collect();
            string json = new JavaScriptSerializer { MaxJsonLength = Int32.MaxValue }.Serialize(report);
            string path = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, filename);
            using (var file = new FileStream(path, FileMode.CreateNew, FileAccess.Write, FileShare.None))
            using (var writer = new StreamWriter(file, new UTF8Encoding(false))) writer.WriteLine(json);
            if (!quiet) MessageBox.Show("采集结束。请带回和此程序在同一文件夹的报告：\n\n" + filename +
                "\n\n匹配的微软 UAC2 播放输出：" + ((List<Row>)report["endpoints"]).Count +
                (report.ContainsKey("collectionError") ? "\n采集遇到错误，报告已记录。" : ""),
                "UAC2 只读采集", MessageBoxButtons.OK, MessageBoxIcon.Information);
            return report.ContainsKey("collectionError") ? 1 : 0;
        } catch (Exception ex) {
            if (!quiet) MessageBox.Show("无法保存报告。请将程序复制到可写的本地文件夹，再双击运行。\n" + Error(ex),
                "UAC2 只读采集", MessageBoxButtons.OK, MessageBoxIcon.Error);
            return 1;
        }
    }
}
