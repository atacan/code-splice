# Metadata contract

The exactness guarantee covers file content bytes. CodeSplice preserves the POSIX
permission bits of an existing target as observed immediately before replacement.
A new target receives `0666 & !startup_umask`.

CodeSplice does not preserve hard-link relationships, ownership, ACLs, extended
attributes, resource forks, timestamps, or platform flags. A changing target with
more than one hard link is rejected. A successful changing report includes
`METADATA_NOT_PRESERVED` to make the exclusions explicit.

Directory parents must already exist. CodeSplice never silently repairs control
directory permissions or target metadata.
