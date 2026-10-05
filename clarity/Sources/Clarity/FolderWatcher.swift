import CoreServices
import Foundation

/// Reports the paths of files that change anywhere under a folder, on the main queue.
/// Uses FSEvents rather than watching a file descriptor, because atomic saves replace the file
/// (a watched descriptor would keep pointing at the old one) and the file may not exist yet.
final class FolderWatcher {
    private var stream: FSEventStreamRef?
    private let onChange: ([String]) -> Void

    init(_ folder: URL, onChange: @escaping ([String]) -> Void) {
        self.onChange = onChange
        var context = FSEventStreamContext(version: 0, info: Unmanaged.passUnretained(self).toOpaque(), retain: nil, release: nil, copyDescription: nil)
        let callback: FSEventStreamCallback = { _, info, _, paths, _, _ in
            guard let info else { return }
            let watcher = Unmanaged<FolderWatcher>.fromOpaque(info).takeUnretainedValue()
            watcher.onChange(Unmanaged<CFArray>.fromOpaque(paths).takeUnretainedValue() as? [String] ?? [])
        }
        let flags = kFSEventStreamCreateFlagFileEvents | kFSEventStreamCreateFlagUseCFTypes | kFSEventStreamCreateFlagNoDefer
        guard let stream = FSEventStreamCreate(
            nil, callback, &context, [folder.path] as CFArray, FSEventStreamEventId(kFSEventStreamEventIdSinceNow), 0.1, FSEventStreamCreateFlags(flags)
        ) else { return }
        self.stream = stream
        FSEventStreamSetDispatchQueue(stream, .main)
        FSEventStreamStart(stream)
    }

    deinit {
        guard let stream else { return }
        FSEventStreamStop(stream)
        FSEventStreamInvalidate(stream)
        FSEventStreamRelease(stream)
    }
}
