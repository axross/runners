#include <sys/wait.h>
#include <unistd.h>
pid_t waitWithShutdown(pid_t pid, int* status, int options);
pid_t forkWithShutdown();
#define waitpid waitWithShutdown
#define fork forkWithShutdown
#define main diagnostic_cli_main
#include "../runner-diagnostics.cpp"
#undef main
#undef waitpid
#undef fork
#include <filesystem>
#include <fstream>
#include <stdexcept>

namespace fs = std::filesystem;
pid_t shutdownWaitOwner = -1, shutdownForkOwner = -1;
int shutdownSignal = SIGTERM, shutdownForks = 0;
std::string shutdownReadyPath;

// delivers a real signal after the launcher's check but before its wait syscall.
pid_t waitWithShutdown(pid_t pid, int* status, int options) {
    if (getpid() == shutdownWaitOwner) {
        shutdownWaitOwner = -1;
        auto deadline = Clock::now() + std::chrono::seconds(1);
        while (readText(shutdownReadyPath).empty() && Clock::now() < deadline) usleep(1000);
        kill(getpid(), shutdownSignal);
    }
    return waitpid(pid, status, options);
}

// injects a pending signal before the runner child's handler reset and exec.
pid_t forkWithShutdown() {
    bool inject = getpid() == shutdownForkOwner && ++shutdownForks == 2;
    pid_t child = fork();
    if (child == 0 && inject) raise(shutdownSignal);
    return child;
}

namespace {
// fails without printing private fixture contents.
void check(bool passed, const char* name) {
    if (!passed) throw std::runtime_error(name);
    std::cout << "PASS " << name << '\n';
}
void put(const fs::path& path, const std::string& text) {
    fs::create_directories(path.parent_path()); std::ofstream(path) << text;
}
bool has(const std::string& text, const std::string& value) { return text.find(value) != std::string::npos; }

// tests independently chosen asymmetric absolute counts and event deltas.
void counters(const fs::path& root, const std::string& marker) {
    auto v2 = root / "v2", v1 = root / "v1", proc = root / "proc";
    put(v2 / "memory.current", "271\n"); put(v2 / "memory.max", "1024\n");
    put(v2 / "memory.events", "oom 7\noom_kill 3\nmax 11\n");
    put(v2 / "cpu.stat", "nr_throttled 23\nthrottled_usec 87\nusage_usec 200\n");
    put(v2 / "cpu.max", "150000 100000\n");
    put(proc / "self/status", "Cpus_allowed_list:\t1,3-4\n");
    put(proc / "41/comm", "clang++\n"); put(proc / "41/status", "PPid:\t17\nVmRSS:\t631 kB\n");
    put(proc / "42/comm", marker); put(proc / "42/cmdline", marker); put(proc / "41/environ", marker);
    std::map<std::string, long long> start;
    auto first = sample(v2.string(), proc.string(), start, false);
    put(v2 / "memory.events", "oom 9\noom_kill 4\nmax 15\n");
    put(v2 / "cpu.stat", "nr_throttled 29\nthrottled_usec 101\nusage_usec 240\n");
    auto last = sample(v2.string(), proc.string(), start, false);
    check(has(first, "cgroup=v2\n") && has(first, "memory.events.oom_kill.absolute=3\n") && has(first, "memory.events.oom_kill.delta=unavailable\n"), "v2 baseline is absolute, not a fabricated zero delta");
    check(has(last, "memory.events.oom.delta=2\n") && has(last, "memory.events.oom_kill.delta=1\n") && has(last, "cpu.nr_throttled.delta=6\n"), "v2 deltas differ from absolute counts");
    check(has(last, "memory.peak.absolute=unavailable\n") && has(last, "memory.swap.max.absolute=unavailable\n"), "missing metrics are unavailable, never zero");
    check(has(last, "cpu.allowed=1,3-4\n") && has(last, "process=compiler pid=41 ppid=17 rss_kib=631\n") && !has(last, marker), "process fields and CPU set are allowlisted");
    put(v1 / "memory/memory.usage_in_bytes", "379\n"); put(v1 / "memory/memory.failcnt", "31\n");
    put(v1 / "memory/memory.oom_control", "under_oom 0\noom_kill 5\n");
    put(v1 / "memory/memory.memsw.limit_in_bytes", "2048\n");
    put(v1 / "memory/memory.limit_in_bytes", "1536\n");
    put(v1 / "memory/memory.memsw.usage_in_bytes", "410\n");
    put(v1 / "cpu,cpuacct/cpu.stat", "nr_throttled 41\nthrottled_time 510\n");
    put(v1 / "cpu,cpuacct/cpu.cfs_quota_us", "-1\n");
    start.clear(); first = sample(v1.string(), proc.string(), start, false);
    put(v1 / "memory/memory.failcnt", "35\n"); put(v1 / "memory/memory.oom_control", "under_oom 0\noom_kill 8\n");
    last = sample(v1.string(), proc.string(), start, false);
    check(has(first, "cgroup=v1\n") && has(first, "cpu.quota_usec.absolute=-1\n") && has(last, "memory.failcnt.delta=4\n") && has(last, "memory.oom_kill.delta=3\n"), "v1 independent fail and kill deltas and unlimited quota");
    check(has(last, "memory.swap.limit_in_bytes.absolute=512\n") && has(last, "memory.swap.usage_in_bytes.absolute=31\n"), "v1 swap is distinct from the memory-plus-swap total");
    check(swapDifference("unavailable", "1536") == "unavailable" && swapDifference("1024", "1536") == "unavailable" && swapDifference("1536", "1536") == "0", "missing swap differs from a measured zero allowance");
    put(v1 / "memory/memory.oom_control", "under_oom 0\n");
    last = sample(v1.string(), proc.string(), start, false);
    check(has(last, "memory.oom_kill.absolute=unavailable\n") && has(last, "memory.oom_kill.delta=unavailable\n"), "missing v1 kill count is not a negative OOM test");
    start.clear(); sample(v1.string(), proc.string(), start, false);
    put(v1 / "memory/memory.oom_control", "oom_kill 17\n"); sample(v1.string(), proc.string(), start, false);
    put(v1 / "memory/memory.oom_control", "oom_kill 19\n"); last = sample(v1.string(), proc.string(), start, false);
    check(has(last, "memory.oom_kill.absolute=19\n") && has(last, "memory.oom_kill.delta=unavailable\n"), "late counters do not fabricate a missing initial baseline");
}

// verifies eligibility and both sides of the raw byte limit without logs on stdout.
void rawRecords(const fs::path& root, const std::string& marker) {
    auto source = root / "reports", target = root / "raw";
    fs::create_directories(source); fs::create_directory(target);
    put(source / "daemon-1.out.log", marker);
    put(source / "unrelated.log", marker); put(source / "heap.hprof", marker);
    fs::create_symlink(source / "daemon-1.out.log", source / "hs_err_pid2.log");
    fs::create_directory_symlink(source, source / "link");
    int dir = directory(target.string());
    RawRecords records(dir);
    records.collect(source.string(), 2, std::regex("(daemon-[0-9]+\\.out|hs_err_pid[0-9]+)\\.log"));
    close(dir);
    check(std::distance(fs::directory_iterator(target), fs::directory_iterator{}) == 1 && !has(records.gaps, marker) && has(records.gaps, "raw_not_regular"), "raw rejects symlinks, unrelated files and dumps");
    fs::remove_all(source); fs::remove_all(target); fs::create_directory(source); fs::create_directory(target);
    auto report = source / "hs_err_pid3.log";
    put(report, ""); fs::resize_file(report, 32 * 1024 * 1024);
    dir = directory(target.string()); RawRecords exact(dir);
    exact.collect(source.string(), 0, std::regex("hs_err_pid[0-9]+\\.log")); close(dir);
    check(fs::file_size(target / "raw-1.log") == 32 * 1024 * 1024, "raw complete record at 32 MiB is retained");
    fs::remove_all(target); fs::create_directory(target); fs::resize_file(report, 32 * 1024 * 1024 + 1);
    dir = directory(target.string()); RawRecords oversized(dir);
    oversized.collect(source.string(), 0, std::regex("hs_err_pid[0-9]+\\.log")); close(dir);
    check(fs::is_empty(target) && has(oversized.gaps, "oversized"), "raw at 32 MiB plus one byte is omitted");
    fs::create_directory_symlink(source, root / "source-link");
    dir = directory(target.string()); RawRecords linked(dir);
    linked.collect((root / "source-link").string(), 0, std::regex(".*")); close(dir);
    check(fs::is_empty(target) && has(linked.gaps, "unavailable"), "raw rejects symlink ancestors");
}

// checks real attach subprocess output and elapsed-time limits with synthetic tools.
void flags(const fs::path& root, const std::string& marker) {
    auto tool = root / "flags.sh";
    put(tool, "#!/bin/sh\nprintf '%s\\n' '-XX:MaxHeapSize=2147483648 -XX:MaxMetaspaceSize=536870912 -XX:ActiveProcessorCount=2 -XX:+UseContainerSupport -XX:OnError=" + marker + "'\n");
    chmod(tool.c_str(), 0700);
    auto text = jvmFlags(getpid(), tool.string());
    check(has(text, "jvm.MaxHeapSize=2147483648\n") && has(text, "jvm.UseContainerSupport=true\n") && !has(text, marker) && !has(text, "OnError"), "numeric and boolean JVM flags only");
    put(tool, "#!/bin/sh\nparent=$PPID\n(sleep 0.05; kill -CONT \"$parent\") >/dev/null 2>&1 &\nkill -STOP \"$parent\"\nprintf '%4096s\\n' ''\nprintf '%s\\n' '-XX:MaxHeapSize=314572800 -XX:ActiveProcessorCount=3'\n");
    text = jvmFlags(getpid(), tool.string());
    check(has(text, "jvm.MaxHeapSize=314572800\n") && has(text, "jvm.ActiveProcessorCount=3\n"), "attach output is drained after tool exit, not truncated at one read");
    put(tool, "#!/bin/sh\nexec sleep 60\n");
    auto start = Clock::now(); text = jvmFlags(getpid(), tool.string());
    check(text.empty() && Clock::now() - start < std::chrono::seconds(2), "attach hang is killed within its timeout");
    put(tool, "#!/bin/sh\nhead -c 20000 /dev/zero\n");
    check(jvmFlags(getpid(), tool.string()).empty(), "oversized tool output is rejected");
}

// exercises production observer bounds with shortened internal limits, not env knobs.
void bounds(const fs::path& root) {
    auto output = root / "bounded"; fs::create_directory(output);
    int dir = directory(output.string());
    stopSignal = 0;
    check(observe(dir, false, 0, 0) == 0, "observer duration stops collection, not a runner"); close(dir);
    check(has(readText((output / "metrics.txt").string()), "gap=observation_duration_limit\n"), "duration bound records a final gap");
    fs::remove(output / "metrics.txt");
    dir = directory(output.string()); check(observe(dir, false, 10, 0, 128) == 0, "small output budget stops observation"); close(dir);
    check(fs::file_size(output / "metrics.txt") <= 128 && has(readText((output / "metrics.txt").string()), "gap=start_sample_unavailable\n"), "output bound is enforced and reported");
    check(IntervalSeconds == 10 && DurationSeconds == 86400 && MetricBudget == 8388608, "production interval, duration and metric budget");
}

fs::path endingRoot;
bool oversizedEnd = false;
std::vector<bool> sampledAttach;

// injects a stop after counters are read, before the periodic sample returns.
std::string endingSample(const std::string&, const std::string&,
                         std::map<std::string, long long>& baseline, bool attach) {
    sampledAttach.push_back(attach);
    auto text = sample((endingRoot / "cgroup").string(), (endingRoot / "proc").string(), baseline, attach);
    if (attach) {
        put(endingRoot / "cgroup/memory.events", "oom_kill 8\n");
        stopped(SIGTERM);
    } else if (oversizedEnd && sampledAttach.size() == 3) text.append(MetricBudget + 1, 'x');
    return text;
}

void finalObservation(const fs::path& root) {
    endingRoot = root / "ending";
    put(endingRoot / "cgroup/memory.current", "271\n");
    for (bool oversized : {false, true}) {
        oversizedEnd = oversized; sampledAttach.clear(); stopSignal = 0;
        put(endingRoot / "cgroup/memory.events", "oom_kill 5\n");
        auto output = endingRoot / (oversized ? "limited" : "complete");
        fs::create_directories(output);
        int dir = directory(output.string());
        check(observe(dir, false, DurationSeconds, IntervalSeconds, MetricBudget, endingSample) == 0, "stop during sample completes finite observation");
        close(dir);
        auto text = readText((output / "metrics.txt").string());
        check(sampledAttach == std::vector<bool>({false, true, false}), "start and end samples never attach, including stop during periodic sampling");
        if (oversized) {
            check(has(text, "gap=final_sample_unavailable\n") && has(text, "final=unavailable\n") && !has(text, "final=observed\n"), "unsaved end sample never claims final observation");
        } else {
            check(has(text, "sample.phase=end\n") && has(text, "memory.events.oom_kill.absolute=8\n") && has(text, "memory.events.oom_kill.delta=3\n") && has(text, "final=observed\n"), "end sample captures counters changed during interrupted sampling");
        }
    }
    stopSignal = 0;
}

int startupDelay = 0, startupCalls = 0;
std::string delayedStartSample(const std::string& cgroup, const std::string& proc,
                               std::map<std::string, long long>& baseline, bool attach) {
    if (startupCalls++ == 0) usleep(startupDelay * 1000);
    return sample(cgroup, proc, baseline, attach);
}

void startupAndSignalWindows(const fs::path& root) {
    for (int delay : {250, 1000}) {
        auto bundle = root / (delay == 250 ? "delayed-start" : "start-timeout");
        auto command = root / (delay == 250 ? "check-start.sh" : "fallback-start.sh");
        put(command, "#!/bin/sh\nif grep -q '^sample.phase=start$' '" + (bundle / "metrics.txt").string() + "'; then exit 0; else exit 7; fi\n");
        chmod(command.c_str(), 0700);
        pid_t parent = fork();
        if (parent == 0) { startupDelay = delay; startupCalls = 0; stopSignal = 0; _exit(launch(command.c_str(), false, bundle.c_str(), delayedStartSample)); }
        auto start = Clock::now();
        int status; waitpid(parent, &status, 0);
        check(WIFEXITED(status) && WEXITSTATUS(status) == (delay == 250 ? 0 : 7), "runner starts only after saved baseline, or preserves fallback result");
        check(Clock::now() - start < std::chrono::seconds(2), "startup observation wait is bounded even when collection stalls");
        if (delay == 1000) check(!has(readText((bundle / "metrics.txt").string()), ".delta=0\n"), "timed-out startup cannot manufacture post-start deltas");
    }
    for (int signal : {SIGTERM, SIGINT}) {
        shutdownSignal = signal;
        auto bundle = root / (signal == SIGTERM ? "wait-term-window" : "wait-int-window");
        auto command = root / "trap-stop.sh";
        shutdownReadyPath = (root / (signal == SIGTERM ? "term-ready" : "int-ready")).string();
        put(command, "#!/bin/sh\ntrap 'exit 23' TERM INT\nprintf ready > '" + shutdownReadyPath + "'\ncount=0\nwhile [ \"$count\" -lt 100 ]; do sleep 0.01; count=$((count+1)); done\nexit 0\n");
        chmod(command.c_str(), 0700);
        pid_t parent = fork();
        if (parent == 0) { shutdownWaitOwner = getpid(); stopSignal = 0; _exit(launch(command.c_str(), false, bundle.c_str())); }
        int status; waitpid(parent, &status, 0);
        check(WIFEXITED(status) && WEXITSTATUS(status) == 23, "signal in check-to-wait window is forwarded, preserving trapped exit");
        bundle = root / (signal == SIGTERM ? "fork-term-window" : "fork-int-window");
        parent = fork();
        if (parent == 0) { shutdownForkOwner = getpid(); shutdownForks = 0; stopSignal = 0; _exit(launch("/bin/true", false, bundle.c_str())); }
        waitpid(parent, &status, 0);
        check(WIFEXITED(status) && WEXITSTATUS(status) == 128 + signal, "pending signal before child handler reset is not swallowed");
    }
}

// runs isolated launcher instances and reaps all test descendants, including abrupt exit.
void lifecycle(const fs::path& root) {
    prctl(PR_SET_CHILD_SUBREAPER, 1);
    auto run = [&](const char* command, const fs::path& bundle, int expected) {
        pid_t child = fork();
        if (child == 0) _exit(launch(command, false, bundle.c_str()));
        int status = 0; waitpid(child, &status, 0);
        check(WIFEXITED(status) && WEXITSTATUS(status) == expected, "runner exit survives observation result");
    };
    run("/bin/true", root / "success", 0);
    check(has(readText((root / "success/metrics.txt").string()), "final=observed\n"), "final sample available before disposal");
    check(std::distance(fs::directory_iterator(root / "success"), fs::directory_iterator{}) == 1, "raw absent by default");
    run("/bin/false", root / "failure", 1);
    fs::create_directory(root / "blocked");
    run("/bin/true", root / "blocked", 0); run("/bin/false", root / "blocked", 1);
    auto command = root / "wait.sh";
    put(command, "#!/bin/sh\nexec sleep 60\n"); chmod(command.c_str(), 0700);
    for (int signal : {SIGTERM, SIGKILL}) {
        auto bundle = root / (signal == SIGTERM ? "graceful" : "abrupt");
        pid_t parent = fork();
        if (parent == 0) _exit(launch(command.c_str(), false, bundle.c_str()));
        std::vector<int> children;
        auto deadline = Clock::now() + std::chrono::seconds(3);
        while (Clock::now() < deadline) {
            std::istringstream ids(readText("/proc/" + std::to_string(parent) + "/task/" + std::to_string(parent) + "/children"));
            children.clear(); int id; while (ids >> id) children.push_back(id);
            if (children.size() == 2 && fs::exists(bundle / "metrics.txt")) break;
            usleep(10000);
        }
        check(children.size() == 2, "launcher owns runner and observer");
        kill(parent, signal); int status; waitpid(parent, &status, 0);
        if (signal == SIGTERM) check(WIFEXITED(status) && WEXITSTATUS(status) == 143, "graceful stop forwards signal and preserves exit");
        else {
            kill(children[1], SIGKILL);
            for (int id : children) waitpid(id, &status, 0);
        }
        for (int id : children) check(kill(id, 0) != 0 && errno == ESRCH, "no surviving observer or test runner");
    }
}
}

int main(int argc, char** argv) {
    if (argc != 3) return 2;
    std::cout << std::unitbuf;
    try {
        fs::path root(argv[1]); std::string marker(argv[2]);
        counters(root, marker); rawRecords(root, marker); flags(root, marker); bounds(root); finalObservation(root); startupAndSignalWindows(root); lifecycle(root);
        std::cout << "All diagnostic observer and lifecycle checks passed.\n";
    } catch (const std::exception& error) { std::cerr << "FAIL " << error.what() << '\n'; return 1; }
}
