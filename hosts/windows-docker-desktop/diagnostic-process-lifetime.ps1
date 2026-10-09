#Requires -Version 5.1
$ErrorActionPreference = 'Stop'

# compiles the built-in Windows lifetime boundary before container work begins.
function Initialize-DiagnosticProcessContainment {
    if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { return }
    if ('Runners.DiagnosticJob' -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
namespace Runners {
    /// <summary>owns a Windows process tree's lifetime without resource limits.</summary>
    public sealed class DiagnosticJob {
        const uint KillOnJobClose = 0x2000;
        const int BasicAccountingInformation = 1, ExtendedLimitInformation = 9;
        [StructLayout(LayoutKind.Sequential)]
        struct BasicLimits {
            public long ProcessUserTime, JobUserTime;
            public uint Flags;
            public UIntPtr MinimumWorkingSet, MaximumWorkingSet;
            public uint ActiveProcessLimit;
            public UIntPtr Affinity;
            public uint Priority, Scheduling;
        }
        [StructLayout(LayoutKind.Sequential)]
        struct IoCounters {
            public ulong ReadOperations, WriteOperations, OtherOperations;
            public ulong ReadBytes, WriteBytes, OtherBytes;
        }
        [StructLayout(LayoutKind.Sequential)]
        struct ExtendedLimits {
            public BasicLimits Basic;
            public IoCounters Io;
            public UIntPtr ProcessMemory, JobMemory, PeakProcessMemory, PeakJobMemory;
        }
        [StructLayout(LayoutKind.Sequential)]
        struct Accounting {
            public long UserTime, KernelTime, PeriodUserTime, PeriodKernelTime;
            public uint PageFaults, TotalProcesses, ActiveProcesses, TerminatedProcesses;
        }
        [DllImport("kernel32.dll", SetLastError = true)]
        static extern IntPtr CreateJobObject(IntPtr attributes, string name);
        [DllImport("kernel32.dll", SetLastError = true)]
        static extern bool SetInformationJobObject(IntPtr job, int kind, ref ExtendedLimits limits, uint size);
        [DllImport("kernel32.dll", SetLastError = true)]
        static extern bool AssignProcessToJobObject(IntPtr job, IntPtr process);
        [DllImport("kernel32.dll", SetLastError = true)]
        static extern bool TerminateJobObject(IntPtr job, uint exitCode);
        [DllImport("kernel32.dll", SetLastError = true)]
        static extern bool QueryInformationJobObject(IntPtr job, int kind, out Accounting accounting, uint size, IntPtr returned);
        [DllImport("kernel32.dll", SetLastError = true)]
        static extern bool CloseHandle(IntPtr handle);
        IntPtr handle;
        /// <summary>creates a non-inheritable owner handle; children cannot break away.</summary>
        /// <exception cref="Win32Exception">Windows cannot establish the lifetime boundary.</exception>
        public DiagnosticJob() {
            handle = CreateJobObject(IntPtr.Zero, null);
            if (handle == IntPtr.Zero) throw new Win32Exception();
            var limits = new ExtendedLimits();
            limits.Basic.Flags = KillOnJobClose;
            if (!SetInformationJobObject(handle, ExtendedLimitInformation, ref limits, (uint)Marshal.SizeOf(typeof(ExtendedLimits)))) {
                int error = Marshal.GetLastWin32Error();
                Close();
                throw new Win32Exception(error);
            }
        }
        public void Assign(IntPtr process) {
            if (!AssignProcessToJobObject(handle, process)) throw new Win32Exception();
        }
        public void Terminate() {
            if (!TerminateJobObject(handle, 1)) throw new Win32Exception();
        }
        public uint ActiveProcesses() {
            Accounting result;
            if (!QueryInformationJobObject(handle, BasicAccountingInformation, out result, (uint)Marshal.SizeOf(typeof(Accounting)), IntPtr.Zero))
                throw new Win32Exception();
            return result.ActiveProcesses;
        }
        public bool Close() {
            if (handle == IntPtr.Zero) return true;
            IntPtr closing = handle;
            handle = IntPtr.Zero;
            return CloseHandle(closing);
        }
    }
}
'@
}
