// The harness's process probe (macOS): for each pid given, prints
// `<pid> <responsible pid>`. WebKit runs each WKWebView's web content,
// networking and GPU work in XPC service processes that launchd starts, so
// they are not in the app's process group; the app is their "responsible"
// process. `lib/proc-sampler.mjs` uses this to find the app's WebKit processes
// for the memory samples.
import Darwin

@_silgen_name("responsibility_get_pid_responsible_for_pid")
func responsiblePid(_ pid: pid_t) -> pid_t

for arg in CommandLine.arguments.dropFirst() {
    guard let pid = pid_t(arg) else { continue }
    print("\(pid) \(responsiblePid(pid))")
}
