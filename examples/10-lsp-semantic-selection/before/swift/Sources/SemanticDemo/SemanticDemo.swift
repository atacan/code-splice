public func keep() -> String {
    "kept"
}
public protocol DisplayNamed {
    var displayName: String { get }
}
public struct Account: DisplayNamed {
    public let displayName: String
    public init(displayName: String) {
        self.displayName = displayName
    }
}
public extension Account {
    func greeting() -> String {
        "Hello, \(displayName)!"
    }
}
public extension DisplayNamed {
    func formattedName() -> String {
        displayName.uppercased()
    }
}
