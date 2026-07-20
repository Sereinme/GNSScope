# ADR-0003: SQLite processing run library

**Status:** Accepted

GNSScope stores its persistent processing run library in one versioned SQLite database under the user's application-data directory. SQLite was selected over per-run JSON or binary directories because runs must be written incrementally, survive crashes, support indexed history queries, preserve immutable configuration snapshots, and migrate across application versions; the database stores derived results and source-file metadata but never copies the source IQ file.
