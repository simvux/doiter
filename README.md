# Doiter - A Cloud Native Language For The Modern World

Doiter uses the running docker daemon as it's virtual memory. It does so by creating a docker container for each allocation/stack-frame. Each structure will also be allocated as a nullable pointer. So taking the following example:

```rust
type Point {
  x: byte,
  y: byte,
};

var point: Point = Point {
  x: 1,
  y: 2,
};
```

The following containers will be created

```bash
<CONTAINER-ONE>: # Container for `point` structure
/x # contains `1` 
/y # contains `2`

<CONTAINER-TWO>: # Container for stack frame of top-level function
/point # contains `<CONTAINER-ONE>`
```

There's two primitive types and literals, `byte` and `char`.
For conditionals, there's the `when` statement. See `samples/hello-world.dr` for details.

The compiler does have error messages with line spans to help you create your own monsters. Enjoy!

### Build instructions

```bash
cargo build --release
# Binary will be in `./target/release`
```

### Run instructions

```bash
doiter samples/hello-world.dr

# If anything goes horribly wrong, you may use this to clean up the mess
doiter --cleanup-all

# To inspect what doiter creates, you can keep the containers and navigate them
doiter --no-cleanup samples/hello-world.dr
docker ps
docker exec <CONTAINER> -it /bin/ash
$ ls /
$ xxd /my-variable-or-field

# If you want to inspect what doiter is doing as it's running
RUST_LOG=doiter=info doiter samples/hello-world.dr
```

### Questions & Answers

**Should I use Doiter?**\
No. This is a terrible idea on every level.

**Is this project vibe-coded?**\
No. Zero AI was used in the making of this monstrosity.

**Why?** \
This project was created as a submission to a hackathon where the goal was to create a "spooky" programming language during October.

I decided that sufficiently cursed would count as spooky.

**How good is the performance?**\
Currently `samples/hello-world.dr` takes about a minute to run on my machine. With the state of the industry, this isn't too bad in comparison.

As there are no strings/arrays it's implemented with a linked list. This means that each character of "Hello World" gets its own docker container. In total it spawns 51 docker containers.

**Woah you must love docker?**\
No. This project is a parody of unnecessary docker use and overabstraction.

**Does Doiter use Garbage Collection?**\
Doiter uses missile allocation. Meaning that no docker containers are cleaned up during execution but all docker containers are automatically removed when the program has completed.

**Are you also working on a real programming language?**\
Yes. Check out `luminalang/lumina`.
