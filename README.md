# wasd5

A Rocket.rs based webapp for my personal site.

## start by creating an admin user

```bash
cargo run --bin cli -- create-user admin mypassword --role admin
```

the run the website:
```bash
## obtain random rescret:
head -c64 /dev/urandom | base64
## add that to Rocket.toml in production
cargo run --bin wasd5 --release
```


and login in to the admin panel to create pages
