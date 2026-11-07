# Pull requests

**Every rule in this file binds each pull request and each push to one.** CI skips a draft pull request and runs on a ready one.

1. If the branch has no pull request, open one with `gh pr create --draft`.
2. You MAY push to a draft pull request at any time.
3. When the working commands pass, run `gh pr ready`.
4. Run `gh pr checks --watch`.

## Issues

**If a pull request fixes a GitHub issue, end its description with `Closes #<number>` for each fixed issue.** The merge closes each issue that a `Closes` line names.

**If a pull request addresses a GitHub issue only in part, name the issue in its description with `Part of #<number>`.** In the same description, state what the issue still needs.

## Release labels

**The labels of a pull request set the version that its merge releases.**

| Label | The change | Example |
| --- | --- | --- |
| none | breaks no program that compiled and ran | `v0.1.4` to `v0.1.5` |
| `release:minor` | before 1.0, makes a program that compiled and ran fail to compile or run | `v0.1.5` to `v0.2.0` |
| `release:major` | is the release that the project owner names as 1.0 | `v0.2.0` to `v1.0.0` |

**If a fix changes emitted Rust that rustc refused, leave the label off.** That fix is a patch.

**The person who merges the pull request applies the release label.** NEVER apply a release label to a pull request you do not merge.

## CI failures

**The `front-end instruction counts` job posts a comparison of instruction counts and counters, and NEVER fails on a rise in an instruction count.** The job fails when a corpus program's allocated bytes or peak live bytes rise more than 1% and no `Counters-Accepted:` trailer in a commit of the pull request names the program. The job reads the base counts from the `front-end base counts` job, which runs beside it. If either job fails in any other step, its tooling broke: treat the failure as a CI failure.

**If the job fails on allocation growth, stop and report the programs and counters it names to the project owner.** NEVER add a `Counters-Accepted:` trailer before the project owner approves the rise.

**If CI fails, run these steps:**

1. Check whether `main` fails the same check, as the next section says. If it does, stop and show the failing output to the project owner.
2. Reproduce the failure with the narrowest working command. If no working command reproduces the failure, read the cause from the CI log.
3. Fix the cause, push, and run `gh pr checks --watch` again.

**NEVER reproduce a CI failure with a command that `AGENTS.md` bans locally.**

**If the `source-format` flake check fails, lay out each file it names with `cargo run --quiet -- fmt <file>`.** The check names each file by its Nix store path. Run the command on the same path in the repository.

**If the `commit builds` job fails, rewrite the commit that its error names:** a later fix commit leaves that commit broken.

**If the same failure survives three fixes, stop and show the failing output to the project owner.**

## The check on `main`

1. Run `gh run list --branch main --workflow CI --status completed --limit 5`.
2. Take the newest run whose conclusion is not `cancelled` and whose title is not a release subject such as `Release v0.8.22`: CI skips every job of a release commit. If no run qualifies, treat `main` as passing.
3. Run `gh run view <id> --json jobs --jq '.jobs[] | {name, conclusion}'` on that run.
4. If the job that failed on the pull request shows `failure` there, `main` fails the same check.

## Probe binaries

**The `flake` job pushes the compiler of each ready pull request and each `main` commit except a release commit to the project's Cachix.** A pull request's binary holds the pull request merged into its base. A release commit changes only the version, so the block below takes a release's first parent as the base.

**If you probe a pull request's unedited code, download the binaries of the pull request and its base with this block:**

```sh
git fetch origin refs/pull/<number>/merge
merge=$(git rev-parse FETCH_HEAD)
base=$(git rev-parse "$merge^1")
[[ $(git log -1 --format=%s "$base") =~ ^Release\ v[0-9]+\.[0-9]+\.[0-9]+$ ]] && base=$(git rev-parse "$base^1")
merge_path=$(nix eval --raw "git+file://$PWD?rev=$merge#default")
base_path=$(nix eval --raw "git+file://$PWD?rev=$base#default")
nix-store --realise "$merge_path" "$base_path"
```

**Run each binary as `"$merge_path/bin/rslisp"` or `"$base_path/bin/rslisp"`.** Nix checks the signature of each download.

**If `nix-store --realise` reports `no substituter that can build it`, run `gh pr checks <number> --watch`, then rerun the block.** If the second run reports the same error, stop and report the commit to the project owner. NEVER build a release binary to replace the download.

**A downloaded binary has no `counters` feature.** Read counters from the `front-end instruction counts` job.

### Without the substituter

**If `nix config show substituters` does not list `https://rslisp.cachix.org`, run this block in place of `nix eval` and `nix-store --realise`:**

```sh
cache=https://rslisp.cachix.org
for commit in "$merge" "$base"; do
  store_path=$(nix eval --raw "git+file://$PWD?rev=$commit#default")
  name=$(basename "$store_path")
  narinfo=$(curl --fail --silent --show-error "$cache/${name%%-*}.narinfo") || break
  nar=$(sed -n 's/^URL: //p' <<< "$narinfo")
  probe=$PWD/target/scratch/probe-$commit
  rm -rf "$probe"
  curl --fail --silent --show-error "$cache/$nar" | zstd --decompress | nix-store --restore "$probe"
  [ "sha256:$(nix-hash --type sha256 --base32 "$probe")" = "$(sed -n 's/^NarHash: //p' <<< "$narinfo")" ] ||
    echo "NarHash mismatch: $commit"
done
```

**This block checks the NarHash and no signature.** If the block prints `NarHash mismatch`, stop and report the commit to the project owner.

**If `curl` reports a 404, run `gh pr checks <number> --watch`, then rerun the block.** If the second run reports a 404, stop and report the commit to the project owner. NEVER build a release binary to replace the download.

**Run each binary as `target/scratch/probe-<commit>/bin/.rslisp-wrapped` inside the dev shell:** `bin/rslisp` points at a store path that this block does not download.

## Callgrind profiles

**The `front-end instruction counts` job uploads the merged pull request's profiles as the `callgrind-profiles` artifact.** GitHub keeps the artifact for seven days. The artifact holds no profiles of the base.

**If you need a ready pull request's profiles, NEVER run callgrind locally.** Download the profiles:

```sh
run=$(gh run list --branch <branch> --workflow CI --event pull_request --limit 1 --json databaseId --jq '.[0].databaseId')
gh run download "$run" --name callgrind-profiles --dir target/scratch/profiles
```

**Read a downloaded profile with `callgrind_annotate <profile>`.**

**If `gh run download` finds no artifact, run `gh pr checks <number> --watch`, then rerun the block.** If the second run finds no artifact, stop and report the pull request to the project owner.
