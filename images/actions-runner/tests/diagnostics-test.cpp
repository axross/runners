#define main diagnostic_cli_main
#include "../runner-diagnostics.cpp"
#undef main
#include <filesystem>
#include <fstream>
#include <stdexcept>

namespace fs = std::filesystem;
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
    check(fs::file_size(output / "metrics.txt") <= 128 && has(readText((output / "metrics.txt").string()), "gap=metric_output_limit\n"), "output bound is enforced and reported");
    check(IntervalSeconds == 10 && DurationSeconds == 86400 && MetricBudget == 8388608, "production interval, duration and metric budget");
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
        counters(root, marker); rawRecords(root, marker); flags(root, marker); bounds(root); lifecycle(root);
        std::cout << "All diagnostic observer and lifecycle checks passed.\n";
    } catch (const std::exception& error) { std::cerr << "FAIL " << error.what() << '\n'; return 1; }
}
