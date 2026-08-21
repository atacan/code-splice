# Metadata contract

The exactness guarantee covers file content bytes. srcmv preserves the POSIX
permission bits of an existing target as observed immediately before replacement.
A new target receives `0666 & !startup_umask`.

srcmv does not preserve hard-link relationships, ownership, ACLs, extended
attributes, resource forks, timestamps, or platform flags. A changing target with
more than one hard link is rejected. A successful changing report includes
`METADATA_NOT_PRESERVED` to make the exclusions explicit.

Directory parents must already exist. srcmv never silently repairs control
directory permissions or target metadata.

The exclusions are part of the v0.1 content-only guarantee, not implementation
defects deferred behind an implicit promise. In particular, APFS resource forks
and flags, Linux extended attributes, ACLs, ownership, and timestamps may be lost
when a target is replaced; power-loss durability is not claimed.
