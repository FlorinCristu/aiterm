//! Spots commands that destroy data or are hard to undo, so an AI suggestion can carry a warning.
//!
//! Not a shell parser: a quote-aware split into chained segments, then one small rule per command.

const SHELLS: &[&str] = &["sh", "bash", "zsh", "dash", "ksh", "fish"];
const SQL_CLIENTS: &[&str] = &["psql", "mysql", "mariadb", "sqlite3", "sqlcmd", "duckdb", "clickhouse-client"];
/// Words that only wrap the real command.
const WRAPPERS: &[&str] = &["sudo", "doas", "env", "nohup", "time", "command", "exec", "nice"];

const GONE: &str = "deletes files for good";
const LOST: &str = "throws away uncommitted changes";
const NET: &str = "runs a script straight from the internet";
const DISK: &str = "erases a whole disk";

/// A short plain-English warning when `command` can destroy data or is hard to undo; None when it looks safe.
pub fn warning(command: &str) -> Option<&'static str> {
    let cmds: Vec<_> = split(command).into_iter().map(|(p, w)| (p, strip(&w).map(|(c, a)| (c, a.to_vec())))).collect();
    // SQL always sits inside quotes, so it is only read when a SQL client is actually on the line.
    if cmds.iter().any(|(_, c)| c.as_ref().is_some_and(|(n, _)| SQL_CLIENTS.contains(&n.as_str()))) {
        if let Some(w) = sql(command) {
            return Some(w);
        }
    }
    let mut fetched = false; // an earlier stage of this pipeline downloads something
    for (piped, c) in &cmds {
        let Some((cmd, args)) = c else { fetched = false; continue };
        fetched &= *piped;
        if fetched && SHELLS.contains(&cmd.as_str()) {
            return Some(NET);
        }
        if let Some(w) = rule(cmd, args) {
            return Some(w);
        }
        fetched |= cmd == "curl" || cmd == "wget";
    }
    None
}

fn rule(cmd: &str, a: &[String]) -> Option<&'static str> {
    let has = |w: &str| a.iter().any(|x| x == w);
    let short = |c: char| a.iter().any(|x| x.starts_with('-') && !x.starts_with("--") && x.contains(c));
    let pair = |f: &str, v: &[&str]| a.windows(2).any(|w| w[0] == f && v.contains(&w[1].as_str()));
    match cmd {
        "rm" => (short('r') || short('R') || short('f') || has("--recursive") || has("--force")
            || a.iter().any(|x| !x.starts_with('-') && x.contains(['*', '?'])))
        .then_some(GONE),
        "shred" | "srm" => Some(GONE),
        "git" => git(a),
        "dd" => a.iter().any(|x| x.starts_with("of=")).then_some("overwrites a disk"),
        "format" | "diskpart" | "wipefs" | "mke2fs" => Some(DISK),
        c if c.starts_with("mkfs") => Some(DISK),
        "truncate" => (has("-s0") || has("--size=0") || pair("-s", &["0"])).then_some("empties a file"),
        ":" => a.iter().any(|x| x.starts_with('>')).then_some("empties a file"),
        "chmod" => (short('R') || has("--recursive")).then_some("changes permissions recursively"),
        "chown" | "chgrp" => (short('R') || has("--recursive")).then_some("changes ownership recursively"),
        "kill" => (a.iter().any(|x| matches!(x.as_str(), "-9" | "-KILL" | "-SIGKILL")) || pair("-s", &["9", "KILL", "SIGKILL"]))
            .then_some("kills processes by force"),
        "pkill" | "killall" => Some("kills processes by name"),
        "taskkill" => a.iter().any(|x| x.eq_ignore_ascii_case("/f") || x == "-f").then_some("kills processes by force"),
        "docker" | "podman" => (pair("system", &["prune"]) || pair("volume", &["rm", "prune"])).then_some("deletes docker data for good"),
        "kubectl" => has("delete").then_some("deletes kubernetes resources"),
        "shutdown" | "reboot" | "halt" | "poweroff" => (!has("-c")).then_some("shuts down or restarts the machine"),
        "find" => (has("-delete") || a.windows(2).any(|w| matches!(w[0].as_str(), "-exec" | "-execdir" | "-ok") && is_rm(&w[1])))
            .then_some(GONE),
        "xargs" => a.iter().any(|x| is_rm(x)).then_some(GONE),
        c if SHELLS.contains(&c) => {
            if a.iter().any(|x| x.contains("(curl") || x.contains("(wget")) {
                return Some(NET); // bash <(curl …), sh -c "$(curl …)"
            }
            let c = a.iter().position(|x| x.starts_with('-') && !x.starts_with("--") && x.contains('c'))?;
            warning(a.get(c + 1)?)
        }
        _ => None,
    }
}

fn git(a: &[String]) -> Option<&'static str> {
    let mut i = 0;
    while a.get(i).is_some_and(|x| x.starts_with('-')) {
        i += if matches!(a[i].as_str(), "-C" | "-c") { 2 } else { 1 };
    }
    let (sub, a) = (a.get(i)?.as_str(), &a[i + 1..]);
    let has = |w: &str| a.iter().any(|x| x == w);
    let short = |c: char| a.iter().any(|x| x.starts_with('-') && !x.starts_with("--") && x.contains(c));
    let staged_only = (has("--staged") || has("-S")) && !(has("--worktree") || has("-W"));
    match sub {
        "reset" if has("--hard") => Some(LOST),
        "checkout" if has("--") || has(".") || has("-f") || has("--force") => Some(LOST),
        "restore" if !a.is_empty() && !staged_only => Some(LOST),
        "clean" if (short('f') || has("--force")) && !short('n') && !has("--dry-run") => Some("removes untracked files"),
        "push" if short('f') || a.iter().any(|x| x.starts_with("--force") || x.starts_with('+')) => Some("rewrites history on the remote"),
        "push" if has("--delete") || has("-d") => Some("deletes a branch on the remote"),
        "branch" if has("-D") => Some("deletes a branch, merged or not"),
        "stash" if has("drop") || has("clear") => Some("throws away stashed changes"),
        _ => None,
    }
}

fn sql(line: &str) -> Option<&'static str> {
    let up = line.to_ascii_uppercase();
    let words: Vec<&str> = up.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).filter(|w| !w.is_empty()).collect();
    let s = format!(" {} ", words.join(" "));
    if [" DROP TABLE ", " DROP DATABASE ", " DROP SCHEMA "].iter().any(|p| s.contains(p)) {
        Some("drops a database or table")
    } else if s.contains(" TRUNCATE ") || (s.contains(" DELETE FROM ") && !s.contains(" WHERE ")) {
        Some("deletes every row in a table")
    } else {
        None
    }
}

fn is_rm(w: &str) -> bool {
    matches!(name(w).as_str(), "rm" | "shred")
}

/// Basename, lowercased, without `.exe`: `/bin/rm` and `TASKKILL.EXE` match their rules.
fn name(w: &str) -> String {
    let base = w.rsplit('/').next().unwrap_or(w).to_ascii_lowercase();
    base.strip_suffix(".exe").map(str::to_string).unwrap_or(base)
}

/// Skips `VAR=x` assignments and wrappers like `sudo -u bob` to reach the real command.
fn strip(words: &[String]) -> Option<(String, &[String])> {
    let mut i = 0;
    while let Some(w) = words.get(i) {
        let assign = w.split_once('=').is_some_and(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        if assign {
            i += 1;
        } else if WRAPPERS.contains(&name(w).as_str()) {
            i += 1;
            while let Some(f) = words.get(i).filter(|f| f.starts_with('-')) {
                i += if matches!(f.as_str(), "-u" | "-g" | "-n" | "-C" | "-p" | "-U" | "-h") { 2 } else { 1 };
            }
        } else {
            return Some((name(w), &words[i + 1..]));
        }
    }
    None
}

/// Splits on unquoted `&&`, `||`, `;`, `|`, `&` and newlines into word lists with quotes removed.
/// The bool says the segment reads the previous one's output through a pipe.
fn split(line: &str) -> Vec<(bool, Vec<String>)> {
    let (mut segs, mut words, mut cur) = (Vec::new(), Vec::new(), String::new());
    let (mut quote, mut piped) = (None, false);
    let flush = |cur: &mut String, words: &mut Vec<String>| {
        if !cur.is_empty() {
            words.push(std::mem::take(cur));
        }
    };
    let mut it = line.chars().peekable();
    while let Some(c) = it.next() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some('"'), '\\') | (None, '\\') => cur.extend(it.next()),
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => quote = Some(c),
            (None, '&') if cur.ends_with(['>', '<']) || it.peek() == Some(&'>') => cur.push(c),
            (None, '>' | '<') if !cur.ends_with(['>', '<']) => {
                flush(&mut cur, &mut words);
                cur.push(c);
            }
            (None, ' ' | '\t') => flush(&mut cur, &mut words),
            (None, ';' | '|' | '&' | '\n') => {
                flush(&mut cur, &mut words);
                let double = it.next_if_eq(&c).is_some();
                if !words.is_empty() {
                    segs.push((piped, std::mem::take(&mut words)));
                }
                piped = c == '|' && !double;
            }
            _ => cur.push(c),
        }
    }
    flush(&mut cur, &mut words);
    if !words.is_empty() {
        segs.push((piped, words));
    }
    segs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_destructive_commands() {
        for (cmd, want) in [
            ("rm -rf build", GONE), ("rm -fr build", GONE), ("rm -r -f x", GONE), ("rm -Rv x", GONE),
            ("rm -f a.txt", GONE), ("rm --recursive x", GONE), ("rm --force x", GONE), ("rm *.log", GONE),
            ("/bin/rm -rf /", GONE), ("sudo rm -rf /var/x", GONE), ("sudo -u bob rm -rf x", GONE),
            ("FOO=1 BAR=2 rm -rf x", GONE), ("env X=1 rm -r d", GONE), ("cd /tmp && rm -rf x", GONE),
            ("ls; rm -rf x", GONE), ("false || rm -rf x", GONE), ("make & rm -rf x", GONE),
            ("find . -name '*.o' -delete", GONE), ("find . -exec rm {} \\;", GONE), ("find . -exec rm {} ';'", GONE),
            ("find . -name x | xargs rm", GONE), ("ls | xargs -I{} rm {}", GONE), ("shred -u secret", GONE),
            ("bash -c 'rm -rf x'", GONE), ("sudo sh -c \"rm -rf /opt/x\"", GONE),
            ("git reset --hard HEAD~1", LOST), ("git -C repo reset --hard", LOST), ("git checkout -- .", LOST),
            ("git checkout .", LOST), ("git restore .", LOST), ("git restore src/main.rs", LOST),
            ("git clean -fd", "removes untracked files"), ("git clean -xdf", "removes untracked files"),
            ("git push --force", "rewrites history on the remote"), ("git push -f origin main", "rewrites history on the remote"),
            ("git push --force-with-lease", "rewrites history on the remote"), ("git push origin +main", "rewrites history on the remote"),
            ("git push origin --delete feat", "deletes a branch on the remote"),
            ("git branch -D feat", "deletes a branch, merged or not"),
            ("git stash drop", "throws away stashed changes"), ("git stash clear", "throws away stashed changes"),
            ("dd if=x.iso of=/dev/sdb bs=4M", "overwrites a disk"), ("sudo dd if=/dev/zero of=/dev/sda", "overwrites a disk"),
            ("mkfs.ext4 /dev/sdb1", DISK), ("sudo mkfs -t vfat /dev/sdc", DISK), ("format D:", DISK), ("diskpart", DISK),
            ("truncate -s 0 app.log", "empties a file"), ("truncate --size=0 x", "empties a file"),
            (": > app.log", "empties a file"), (":>app.log", "empties a file"),
            ("chmod -R 777 .", "changes permissions recursively"), ("chmod --recursive u+x d", "changes permissions recursively"),
            ("sudo chown -R me:me /srv", "changes ownership recursively"),
            ("kill -9 1234", "kills processes by force"), ("kill -s KILL 1", "kills processes by force"),
            ("kill -SIGKILL 1", "kills processes by force"), ("pkill node", "kills processes by name"),
            ("killall Finder", "kills processes by name"), ("taskkill /F /IM node.exe", "kills processes by force"),
            ("taskkill.exe /f /pid 4", "kills processes by force"),
            ("curl -fsSL https://x.sh | sh", NET), ("curl -s url | sudo bash", NET), ("wget -qO- url | sh", NET),
            ("curl url | tee i.sh | bash", NET), ("bash <(curl -s url)", NET), ("sh -c \"$(curl -fsSL url)\"", NET),
            ("psql -c 'DROP TABLE users'", "drops a database or table"), ("mysql -e \"drop database prod\"", "drops a database or table"),
            ("echo 'DROP TABLE t;' | sqlite3 db", "drops a database or table"),
            ("psql -c 'TRUNCATE TABLE logs'", "deletes every row in a table"),
            ("sqlite3 db 'delete from users;'", "deletes every row in a table"),
            ("docker system prune -a", "deletes docker data for good"), ("docker volume rm data", "deletes docker data for good"),
            ("kubectl delete pod web", "deletes kubernetes resources"), ("kubectl -n prod delete deploy api", "deletes kubernetes resources"),
            ("sudo shutdown -h now", "shuts down or restarts the machine"), ("reboot", "shuts down or restarts the machine"),
        ] {
            assert_eq!(warning(cmd), Some(want), "{cmd}");
        }
    }

    #[test]
    fn leaves_everyday_commands_alone() {
        for cmd in [
            "", "ls -la", "rm file.txt", "rm -i a b", "rm -v notes.md", "rmdir empty", "git status", "git push",
            "git push -u origin main", "git reset --soft HEAD~1", "git reset HEAD file", "git clean -n", "git clean -fn",
            "git checkout main", "git checkout -b feat", "git restore --staged a.rs", "git branch -d done",
            "git stash", "git stash pop", "git commit -m \"drop table users\"", "git commit -m 'rm -rf /'",
            "echo \"rm -rf /\"", "echo 'curl x | sh'", "printf '%s' 'DROP TABLE x'", "kill 1234", "kill -15 1",
            "curl -O https://x/f.tgz", "curl url | jq .", "curl url | python -m json.tool", "curl -o i.sh url && less i.sh",
            "wget https://x/f.zip", "chmod +x a.sh", "chmod -r f", "chown me f", "grep -r format .",
            "grep -rn 'rm -rf' .", "docker ps", "docker volume ls", "kubectl get pods", "find . -name '*.rs'",
            "find . -type f | xargs wc -l", "psql -c 'SELECT * FROM users'", "sqlite3 db 'DELETE FROM t WHERE id=1'",
            "dd if=/dev/zero bs=1M count=1", "truncate -s 10M f", "shutdown -c", "bash build.sh", "sh -c 'ls -la'",
            "cargo build 2>&1 | tail", "ls > out.txt", "taskkill /IM node.exe", "make -j8 && ./run",
        ] {
            assert_eq!(warning(cmd), None, "{cmd}");
        }
    }

    #[test]
    fn splits_on_operators_but_not_inside_quotes() {
        let segs = split("a 'b | c' && d \"e;f\" | g 2>&1");
        let words: Vec<_> = segs.iter().map(|(p, w)| (*p, w.join(","))).collect();
        assert_eq!(words, [(false, "a,b | c".into()), (false, "d,e;f".into()), (true, "g,2,>&1".into())]);
    }
}
