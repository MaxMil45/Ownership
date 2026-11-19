-- Runs once, when the oe-pgdata volume is first created.
-- A separate, empty database for `cargo test -p oe-store --test postgres -- --ignored`.
CREATE DATABASE ownership_test OWNER oe;
