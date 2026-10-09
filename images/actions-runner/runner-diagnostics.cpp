#include <algorithm>
#include <cerrno>
#include <chrono>
#include <csignal>
#include <dirent.h>
#include <fcntl.h>
#include <iostream>
#include <map>
#include <poll.h>
#include <regex>
#include <sstream>
#include <string>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>
#include <vector>

namespace {
using Clock = std::chrono::steady_clock;
constexpr size_t MetricBudget = 8 * 1024 * 1024;
constexpr size_t RawBudget = 32 * 1024 * 1024;
constexpr size_t FinalMetricReserve = 4096;
constexpr size_t AttachOutputBudget = 16384;
constexpr int AttachTimeoutMilliseconds = 500;
constexpr int ProcessLimit = 128;
constexpr int JvmAttachLimit = 4;
constexpr int RawRecordLimit = 128;
constexpr int RawCollectionSeconds = 3;
constexpr int WorkspaceDepth = 2;
constexpr int ObserverFinalizationSeconds = 6;
constexpr int StartupMilliseconds = 500;
constexpr int DurationSeconds = 24 * 60 * 60;
constexpr int IntervalSeconds = 10;
constexpr int ScanLimit = 1024;
volatile sig_atomic_t stopSignal = 0;
void stopped(int signal) { stopSignal = signal; }

// reads only a finite prefix; proc and cgroup files need no external commands.
std::string readText(const std::string& path, size_t limit = 16384) {
    int fd = open(path.c_str(), O_RDONLY | O_NONBLOCK | O_CLOEXEC | O_NOFOLLOW);
    if (fd < 0) return {};
    std::string text(limit + 1, '\0');
    ssize_t count = read(fd, text.data(), text.size());
    close(fd);
    if (count < 0 || static_cast<size_t>(count) > limit) return {};
    text.resize(count);
    return text;
}

std::string numeric(const std::string& value) {
    static const std::regex number("-?[0-9]{1,20}|max");
    return std::regex_match(value, number) ? value : "unavailable";
}

std::string scalar(const std::string& path) {
    std::istringstream input(readText(path));
    std::string value, extra;
    input >> value;
    if (input >> extra) return "unavailable";
    return numeric(value);
}

// v1 memsw includes RAM; missing or inconsistent inputs cannot imply zero swap.
std::string swapDifference(const std::string& combined, const std::string& memory) {
    try {
        auto total = std::stoll(combined), ram = std::stoll(memory);
        if (ram >= 0 && total >= ram) return std::to_string(total - ram);
    } catch (const std::exception&) {}
    return "unavailable";
}

// discards unknown keys and values rather than exposing diagnostic-tool output.
std::map<std::string, std::string> fields(const std::string& text) {
    std::map<std::string, std::string> result;
    std::istringstream input(text);
    std::string key, value;
    while (input >> key >> value) result[key] = numeric(value);
    return result;
}

std::string field(const std::map<std::string, std::string>& values, const std::string& key) {
    auto found = values.find(key);
    return found == values.end() ? "unavailable" : found->second;
}

// caps directory enumeration, including directories filled with unrelated files.
std::vector<std::string> names(int fd, bool& limited, bool& unavailable) {
    std::vector<std::string> result;
    int duplicate = dup(fd);
    DIR* dir = fdopendir(duplicate);
    if (!dir) {
        if (duplicate >= 0) close(duplicate);
        unavailable = true;
        return result;
    }
    int count = 0;
    for (;;) {
        errno = 0;
        dirent* entry = readdir(dir);
        if (!entry) { unavailable = errno != 0; break; }
        if (++count > ScanLimit) { limited = true; break; }
        std::string name = entry->d_name;
        if (name != "." && name != "..") result.push_back(name);
    }
    closedir(dir);
    return result;
}

// traverses each component by directory descriptor, rejecting symlink ancestors.
int directory(const std::string& path) {
    int fd = open(path[0] == '/' ? "/" : ".", O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    std::istringstream input(path);
    std::string part;
    while (std::getline(input, part, '/')) {
        if (part.empty() || part == ".") continue;
        if (part == "..") { close(fd); return -1; }
        int next = openat(fd, part.c_str(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
        close(fd);
        fd = next;
        if (fd < 0) break;
    }
    return fd;
}

// a JVM attach can fail or hang; neither its stderr nor unfiltered stdout escapes.
std::string jvmFlags(int pid, const std::string& tool) {
    int pipes[2];
    if (pipe2(pipes, O_CLOEXEC) != 0) return {};
    pid_t child = fork();
    if (child == 0) {
        setpgid(0, 0);
        dup2(pipes[1], STDOUT_FILENO);
        int null = open("/dev/null", O_WRONLY);
        dup2(null, STDERR_FILENO);
        close(pipes[0]); close(pipes[1]);
        std::string id = std::to_string(pid);
        execl(tool.c_str(), tool.c_str(), id.c_str(), "VM.flags", static_cast<char*>(nullptr));
        _exit(127);
    }
    close(pipes[1]);
    if (child < 0) { close(pipes[0]); return {}; }
    setpgid(child, child);
    fcntl(pipes[0], F_SETFL, O_NONBLOCK);
    std::string output;
    auto deadline = Clock::now() + std::chrono::milliseconds(AttachTimeoutMilliseconds);
    int status = 0;
    bool exited = false, drained = false;
    while (Clock::now() < deadline && output.size() < AttachOutputBudget) {
        char buffer[1024];
        ssize_t count = read(pipes[0], buffer, sizeof(buffer));
        if (count > 0) output.append(buffer, count);
        if (count == 0) drained = true;
        if (!exited && waitpid(child, &status, WNOHANG) == child) exited = true;
        if (exited && drained) break;
        pollfd poller{pipes[0], POLLIN, 0};
        poll(&poller, 1, 10);
    }
    kill(-child, SIGKILL);
    if (!exited) waitpid(child, &status, 0);
    close(pipes[0]);
    if (!exited || !drained || !WIFEXITED(status) || WEXITSTATUS(status) != 0 || output.size() >= AttachOutputBudget) return {};
    static const std::regex flag("-XX:(?:([+-])(UseContainerSupport)|((?:InitialHeapSize|MaxHeapSize|MaxMetaspaceSize|ActiveProcessorCount))=(-?[0-9]{1,20}))(?:[[:space:]]|$)");
    std::ostringstream filtered;
    for (std::sregex_iterator it(output.begin(), output.end(), flag), end; it != end; ++it) {
        if ((*it)[2].matched) filtered << "jvm." << (*it)[2] << '=' << ((*it)[1] == "+" ? "true" : "false") << '\n';
        else filtered << "jvm." << (*it)[3] << '=' << (*it)[4] << '\n';
    }
    return filtered.str();
}

// emits recognized categories only, never cmdline, environment or arbitrary names.
std::string processes(const std::string& proc, bool attach) {
    std::ostringstream output;
    int fd = directory(proc);
    if (fd < 0) return "processes=unavailable\n";
    bool limited = false, unavailable = false;
    auto entries = names(fd, limited, unavailable);
    close(fd);
    const std::map<std::string, std::string> categories{
        {"java", "jvm"}, {"clang", "compiler"}, {"clang++", "compiler"},
        {"cc1", "compiler"}, {"cc1plus", "compiler"}, {"ld", "linker"},
        {"ld.lld", "linker"}, {"ninja", "native-build"}, {"cmake", "native-build"}};
    int selected = 0, attached = 0;
    for (const auto& id : entries) {
        if (!std::regex_match(id, std::regex("[0-9]{1,9}"))) continue;
        std::string comm = readText(proc + '/' + id + "/comm", 64);
        if (!comm.empty() && comm.back() == '\n') comm.pop_back();
        auto category = categories.find(comm);
        if (category == categories.end()) continue;
        if (++selected > ProcessLimit) { limited = true; break; }
        std::istringstream status(readText(proc + '/' + id + "/status"));
        std::string line, parent = "unavailable", rss = "unavailable";
        while (std::getline(status, line)) {
            std::istringstream row(line);
            std::string key, value; row >> key >> value;
            if (key == "PPid:") parent = numeric(value);
            if (key == "VmRSS:") rss = numeric(value);
        }
        output << "process=" << category->second << " pid=" << id << " ppid=" << parent << " rss_kib=" << rss << '\n';
        if (comm == "java") {
            std::string flags;
            if (attach && attached++ < JvmAttachLimit) {
                flags = jvmFlags(std::stoi(id), "/opt/hostedtoolcache/Java_Temurin-Hotspot_jdk/17.0.20-101/x64/bin/jcmd");
            }
            output << "jvm.pid=" << id << '\n' << (flags.empty() ? "jvm.flags=unavailable\n" : flags);
        }
    }
    if (limited) output << "gap=process_scan_limit\n";
    if (unavailable) output << "gap=process_scan_unavailable\n";
    return output.str();
}

// records absolute counters and baseline deltas separately; no process attribution.
std::string sample(const std::string& root, const std::string& proc,
                   std::map<std::string, long long>& baseline, bool attach) {
    std::ostringstream output;
    const bool v2 = !readText(root + "/cgroup.controllers").empty() || access((root + "/memory.current").c_str(), F_OK) == 0;
    const bool v1 = access((root + "/memory/memory.usage_in_bytes").c_str(), F_OK) == 0;
    output << "cgroup=" << (v2 ? "v2" : v1 ? "v1" : "unavailable") << '\n';
    std::map<std::string, std::string> values;
    if (v2) {
        for (const auto& item : {"memory.current", "memory.peak", "memory.max", "memory.swap.current", "memory.swap.max"}) values[item] = scalar(root + '/' + item);
        auto events = fields(readText(root + "/memory.events"));
        for (const auto& item : {"low", "high", "max", "oom", "oom_kill", "oom_group_kill"}) values[std::string("memory.events.") + item] = field(events, item);
        auto cpu = fields(readText(root + "/cpu.stat"));
        for (const auto& item : {"usage_usec", "user_usec", "system_usec", "nr_periods", "nr_throttled", "throttled_usec"}) values[std::string("cpu.") + item] = field(cpu, item);
        std::istringstream quota(readText(root + "/cpu.max"));
        std::string amount, period; quota >> amount >> period;
        values["cpu.quota_usec"] = numeric(amount); values["cpu.period_usec"] = numeric(period);
    } else {
        for (const auto& item : {"usage_in_bytes", "max_usage_in_bytes", "limit_in_bytes", "failcnt", "memsw.usage_in_bytes", "memsw.limit_in_bytes"}) values[std::string("memory.") + item] = scalar(root + "/memory/memory." + item);
        values["memory.swap.limit_in_bytes"] = swapDifference(values["memory.memsw.limit_in_bytes"], values["memory.limit_in_bytes"]);
        values["memory.swap.usage_in_bytes"] = swapDifference(values["memory.memsw.usage_in_bytes"], values["memory.usage_in_bytes"]);
        auto oom = fields(readText(root + "/memory/memory.oom_control"));
        values["memory.oom_kill"] = field(oom, "oom_kill");
        values["memory.under_oom"] = field(oom, "under_oom");
        std::string cpuRoot = root + "/cpu";
        if (access((cpuRoot + "/cpu.stat").c_str(), F_OK) != 0) cpuRoot = root + "/cpu,cpuacct";
        auto cpu = fields(readText(cpuRoot + "/cpu.stat"));
        for (const auto& item : {"nr_periods", "nr_throttled", "throttled_time"}) values[std::string("cpu.") + item] = field(cpu, item);
        values["cpu.usage_ns"] = scalar(cpuRoot + "/cpuacct.usage");
        if (values["cpu.usage_ns"] == "unavailable") values["cpu.usage_ns"] = scalar(root + "/cpuacct/cpuacct.usage");
        values["cpu.quota_usec"] = scalar(cpuRoot + "/cpu.cfs_quota_us");
        values["cpu.period_usec"] = scalar(cpuRoot + "/cpu.cfs_period_us");
    }
    for (const auto& [key, value] : values) {
        output << key << ".absolute=" << value << '\n';
        if (key.find("events.") != std::string::npos || key == "memory.failcnt" || key == "memory.oom_kill" || (key.find("cpu.") == 0 && key.find("quota") == std::string::npos && key.find("period_usec") == std::string::npos)) {
            std::string delta = "unavailable";
            auto [start, initial] = baseline.emplace(key, -1);
            try {
                if (value != "unavailable" && value != "max") {
                    long long current = std::stoll(value);
                    if (initial && current >= 0) start->second = current;
                    else if (start->second >= 0 && current >= start->second) delta = std::to_string(current - start->second);
                }
            } catch (const std::exception&) {}
            output << key << ".delta=" << delta << '\n';
        }
    }
    std::string allowed;
    std::istringstream status(readText(proc + "/self/status"));
    std::string line;
    while (std::getline(status, line)) if (line.find("Cpus_allowed_list:") == 0) {
        std::istringstream row(line.substr(18)); row >> allowed;
    }
    if (!std::regex_match(allowed, std::regex("[0-9,-]{1,4096}"))) allowed = "unavailable";
    output << "cpu.allowed=" << allowed << '\n' << processes(proc, attach);
    return output.str();
}

// stores complete non-symlink reports through pinned directory descriptors.
class RawRecords {
    int output;
    size_t used = 0;
    int sequence = 0;
    Clock::time_point deadline;
public:
    explicit RawRecords(int fd) : output(fd), deadline(Clock::now() + std::chrono::seconds(RawCollectionSeconds)) {}
    std::string gaps;
    void collect(const std::string& path, int depth, const std::regex& pattern) {
        int dir = directory(path);
        if (dir < 0) { gap("raw_location_unavailable"); return; }
        walk(dir, depth, pattern);
        close(dir);
    }
private:
    void gap(const std::string& code) {
        std::string line = "gap=" + code + '\n';
        if (gaps.find(line) == std::string::npos) gaps += line;
    }
    // a bounded traversal is intentionally not an arbitrary workspace search.
    void walk(int dir, int depth, const std::regex& pattern) {
        bool limited = false, unavailable = false;
        for (const auto& name : names(dir, limited, unavailable)) {
            if (Clock::now() >= deadline || sequence >= RawRecordLimit) { limited = true; break; }
            struct stat info{};
            if (fstatat(dir, name.c_str(), &info, AT_SYMLINK_NOFOLLOW) != 0) { gap("raw_metadata_unavailable"); continue; }
            if (S_ISDIR(info.st_mode) && depth > 0) {
                int child = openat(dir, name.c_str(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
                if (child < 0) { gap("raw_location_unavailable"); continue; }
                if (depth == WorkspaceDepth) {
                    int checkout = openat(child, name.c_str(), O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
                    if (checkout >= 0) { walk(checkout, 0, pattern); close(checkout); }
                    else if (errno != ENOENT) { gap("raw_location_unavailable"); }
                } else {
                    walk(child, depth - 1, pattern);
                }
                close(child);
            } else if (depth == 0 && std::regex_match(name, pattern)) {
                if (!S_ISREG(info.st_mode)) { gap("raw_not_regular"); continue; }
                copy(dir, name);
            }
        }
        if (limited) gap("raw_scan_limit");
        if (unavailable) gap("raw_scan_unavailable");
    }
    // pins the source inode and removes an incomplete generated destination only.
    void copy(int dir, const std::string& name) {
        int source = openat(dir, name.c_str(), O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC);
        struct stat before{};
        if (source < 0 || fstat(source, &before) != 0 || !S_ISREG(before.st_mode) || before.st_size < 0 || static_cast<size_t>(before.st_size) > RawBudget - used) {
            if (source >= 0) close(source);
            gap("raw_omitted_or_oversized"); return;
        }
        std::string target = "raw-" + std::to_string(++sequence) + ".log";
        int dest = openat(output, target.c_str(), O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0600);
        size_t copied = 0;
        bool okay = dest >= 0;
        char buffer[65536];
        while (okay && copied < static_cast<size_t>(before.st_size) && Clock::now() < deadline) {
            size_t wanted = std::min(sizeof(buffer), static_cast<size_t>(before.st_size) - copied);
            ssize_t count = read(source, buffer, wanted);
            if (count <= 0 || write(dest, buffer, count) != count) { okay = false; break; }
            copied += count;
        }
        struct stat after{};
        okay = okay && copied == static_cast<size_t>(before.st_size) && fstat(source, &after) == 0 && before.st_size == after.st_size && before.st_mtim.tv_sec == after.st_mtim.tv_sec && before.st_mtim.tv_nsec == after.st_mtim.tv_nsec;
        close(source);
        if (dest >= 0) close(dest);
        if (okay) used += copied;
        else { unlinkat(output, target.c_str(), 0); gap("raw_copy_failed"); }
    }
};

// bounds observer work independently of the runner's lifetime and result.
int observe(int dir, bool raw, int duration = DurationSeconds, int interval = IntervalSeconds,
            size_t budget = MetricBudget, decltype(&sample) collect = sample, int ready = -1) {
    int metrics = openat(dir, "metrics.txt", O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0600);
    if (metrics < 0) return 1;
    auto start = Clock::now();
    std::map<std::string, long long> baseline;
    size_t used = 0;
    auto append = [&](const std::string& text) {
        if (text.size() > budget - used) return false;
        if (write(metrics, text.data(), text.size()) != static_cast<ssize_t>(text.size())) return false;
        used += text.size(); return true;
    };
    bool bounded = false;
    std::string initial = "sample.phase=start\nsample.elapsed_seconds=0\n" + collect("/sys/fs/cgroup", "/proc", baseline, false);
    if (initial.size() + FinalMetricReserve > budget - used || !append(initial)) {
        append("gap=start_sample_unavailable\n");
        bounded = true;
    }
    if (ready >= 0) {
        char saved = '1';
        bool notified = !bounded && write(ready, &saved, 1) == 1;
        close(ready);
        if (!notified) { close(metrics); return 1; }
    }
    while (!bounded) {
        if (stopSignal) break;
        auto next = Clock::now() + std::chrono::seconds(interval);
        std::string text = "sample.phase=periodic\nsample.elapsed_seconds=" + std::to_string(std::chrono::duration_cast<std::chrono::seconds>(Clock::now() - start).count()) + '\n' + collect("/sys/fs/cgroup", "/proc", baseline, true);
        if (text.size() + FinalMetricReserve > budget - used) { append("gap=metric_output_limit\n"); bounded = true; break; }
        if (!append(text)) { close(metrics); return 1; }
        if (stopSignal) break;
        while (!stopSignal && Clock::now() < next) usleep(100000);
        if (Clock::now() - start >= std::chrono::seconds(duration)) { append("gap=observation_duration_limit\n"); bounded = true; break; }
    }
    if (!bounded) {
        std::string text = "sample.phase=end\nsample.elapsed_seconds=" + std::to_string(std::chrono::duration_cast<std::chrono::seconds>(Clock::now() - start).count()) + '\n' + collect("/sys/fs/cgroup", "/proc", baseline, false);
        if (text.size() + FinalMetricReserve > budget - used || !append(text)) {
            append("gap=final_sample_unavailable\n");
            bounded = true;
        }
    }
    if (raw) {
        RawRecords records(dir);
        records.collect("/home/runner/.gradle/daemon", 1, std::regex("daemon-[0-9]+\\.out\\.log"));
        records.collect("/home/runner/_work", WorkspaceDepth, std::regex("hs_err_pid[0-9]+\\.log"));
        records.collect("/home/runner", 0, std::regex("hs_err_pid[0-9]+\\.log"));
        records.collect("/tmp", 0, std::regex("hs_err_pid[0-9]+\\.log"));
        append(records.gaps.substr(0, 2048));
    }
    append(bounded ? "final=unavailable\n" : "final=observed\n");
    close(metrics);
    return 0;
}

// waits only for a saved pre-run baseline; a late observer cannot invent deltas.
pid_t startObserver(int dir, bool raw, decltype(&sample) collect) {
    auto deadline = Clock::now() + std::chrono::milliseconds(StartupMilliseconds);
    int ready[2];
    if (pipe2(ready, O_NONBLOCK | O_CLOEXEC) != 0) return -1;
    pid_t parent = getpid();
    pid_t observer = fork();
    if (observer == 0) {
        close(ready[0]);
        prctl(PR_SET_PDEATHSIG, SIGTERM);
        if (getppid() != parent) stopSignal = SIGTERM;
        _exit(observe(dir, raw, DurationSeconds, IntervalSeconds, MetricBudget, collect, ready[1]));
    }
    close(ready[1]);
    bool saved = false;
    while (observer > 0 && Clock::now() < deadline) {
        char value;
        ssize_t count = read(ready[0], &value, 1);
        if (count == 1) { saved = value == '1'; break; }
        if (count == 0) break;
        pollfd poller{ready[0], POLLIN, 0};
        poll(&poller, 1, 10);
    }
    close(ready[0]);
    if (!saved) {
        if (observer > 0) kill(observer, SIGKILL);
        std::cerr << "Diagnostic gap: start observation unavailable; counter deltas unavailable.\n";
    }
    return observer;
}

// forwards shutdown to the ordinary runner, then reaps a finite-lived observer.
int launch(const char* command, bool raw, const char* bundle = "/tmp/runner-diagnostics",
           decltype(&sample) collect = sample) {
    struct sigaction action{};
    action.sa_handler = stopped;
    sigemptyset(&action.sa_mask);
    sigaction(SIGTERM, &action, nullptr); sigaction(SIGINT, &action, nullptr);
    int dir = -1;
    if (mkdir(bundle, 0700) == 0) dir = directory(bundle);
    pid_t observer = -1;
    if (dir >= 0) {
        observer = startObserver(dir, raw, collect);
        close(dir);
    }
    if (observer < 0) std::cerr << "Diagnostic gap: observer unavailable.\n";
    sigset_t shutdown, previous;
    sigemptyset(&shutdown); sigaddset(&shutdown, SIGTERM); sigaddset(&shutdown, SIGINT);
    sigprocmask(SIG_BLOCK, &shutdown, &previous);
    pid_t runner = fork();
    if (runner == 0) {
        signal(SIGTERM, SIG_DFL); signal(SIGINT, SIG_DFL);
        sigprocmask(SIG_SETMASK, &previous, nullptr);
        execl(command, command, static_cast<char*>(nullptr)); _exit(127);
    }
    sigprocmask(SIG_SETMASK, &previous, nullptr);
    int status = 0;
    if (runner < 0) status = 127 << 8;
    else {
        int forwarded = 0;
        for (;;) {
            if (stopSignal && stopSignal != forwarded) { forwarded = stopSignal; kill(runner, forwarded); }
            pid_t waited = waitpid(runner, &status, WNOHANG);
            if (waited == runner) break;
            if (waited < 0 && errno != EINTR) { status = 127 << 8; break; }
            usleep(10000);
        }
    }
    if (observer > 0) {
        kill(observer, SIGTERM);
        auto deadline = Clock::now() + std::chrono::seconds(ObserverFinalizationSeconds);
        int observerStatus = 0;
        bool reaped = false;
        while (Clock::now() < deadline) {
            if (waitpid(observer, &observerStatus, WNOHANG) == observer) { reaped = true; break; }
            usleep(10000);
        }
        if (!reaped) { kill(observer, SIGKILL); waitpid(observer, &observerStatus, 0); }
        if (!reaped || !WIFEXITED(observerStatus) || WEXITSTATUS(observerStatus) != 0) std::cerr << "Diagnostic gap: observer failed or final sample unavailable.\n";
    }
    return WIFEXITED(status) ? WEXITSTATUS(status) : 128 + WTERMSIG(status);
}
}

int main(int argc, char** argv) {
    if (argc == 2 && std::string(argv[1]) == "raw-run") return launch("/home/runner/run.sh", true);
    if (argc == 1) return launch("/home/runner/run.sh", false);
    return 2;
}
