using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class UmDxgiLuidAudit {
 [StructLayout(LayoutKind.Sequential)] struct Luid { public uint LowPart; public int HighPart; }
 [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct Desc1 {
  [MarshalAs(UnmanagedType.ByValTStr, SizeConst=128)] public string Description;
  public uint VendorId, DeviceId, SubSysId, Revision;
  public UIntPtr DedicatedVideoMemory, DedicatedSystemMemory, SharedSystemMemory;
  public Luid AdapterLuid; public uint Flags;
 }
 [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int EnumAdapters1Fn(IntPtr factory, uint index, out IntPtr adapter);
 [UnmanagedFunctionPointer(CallingConvention.StdCall)] delegate int GetDesc1Fn(IntPtr adapter, out Desc1 desc);
 [DllImport("dxgi.dll", ExactSpelling=true)] static extern int CreateDXGIFactory1(ref Guid iid, out IntPtr factory);
 public sealed class Adapter {
  public uint Index,VendorId,DeviceId,Flags,LuidHigh,LuidLow;
  public string Description,LuidKey;
  public bool Software,Remote;
  public ulong DedicatedVideoMemory,DedicatedSystemMemory,SharedSystemMemory;
 }
 static IntPtr Slot(IntPtr instance,int index) { return Marshal.ReadIntPtr(Marshal.ReadIntPtr(instance),index*IntPtr.Size); }
 public static Adapter[] Read() {
  var list=new List<Adapter>(); IntPtr factory=IntPtr.Zero;
  Guid iid=new Guid("770aae78-f26f-4dba-a829-253c83d1b387");
  Marshal.ThrowExceptionForHR(CreateDXGIFactory1(ref iid,out factory));
  try {
   var enumerate=(EnumAdapters1Fn)Marshal.GetDelegateForFunctionPointer(Slot(factory,12),typeof(EnumAdapters1Fn));
   for(uint index=0;index<64;index++) {
    IntPtr adapter; int result=enumerate(factory,index,out adapter);
    if(result==unchecked((int)0x887A0002)) break;
    Marshal.ThrowExceptionForHR(result);
    try {
     var getDesc=(GetDesc1Fn)Marshal.GetDelegateForFunctionPointer(Slot(adapter,10),typeof(GetDesc1Fn));
     Desc1 d; Marshal.ThrowExceptionForHR(getDesc(adapter,out d));
     var row=new Adapter();row.Index=index;row.Description=d.Description;
     row.VendorId=d.VendorId;row.DeviceId=d.DeviceId;row.Flags=d.Flags;
     row.Software=(d.Flags&2)!=0;row.Remote=(d.Flags&1)!=0;
     row.LuidHigh=unchecked((uint)d.AdapterLuid.HighPart);row.LuidLow=d.AdapterLuid.LowPart;
     row.LuidKey=row.LuidHigh.ToString("X8")+":"+row.LuidLow.ToString("X8");
     row.DedicatedVideoMemory=d.DedicatedVideoMemory.ToUInt64();row.DedicatedSystemMemory=d.DedicatedSystemMemory.ToUInt64();row.SharedSystemMemory=d.SharedSystemMemory.ToUInt64();
     list.Add(row);
    } finally {Marshal.Release(adapter);}
   }
  } finally {Marshal.Release(factory);}
  return list.ToArray();
 }
}
