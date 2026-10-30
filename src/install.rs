//! When bash says "command not found", work out how to install that command on this OS.

use crate::detect::Detector;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Manager {
    Winget,
    Brew,
    Apt,
    Dnf,
    Pacman,
    Zypper,
    Apk,
}

impl Manager {
    /// The package manager this machine uses, judged by the OS and what is on PATH.
    pub fn detect(detector: &Detector) -> Manager {
        if cfg!(windows) {
            return Manager::Winget;
        }
        if cfg!(target_os = "macos") {
            return Manager::Brew;
        }
        for (cmd, m) in [
            ("apt-get", Manager::Apt),
            ("dnf", Manager::Dnf),
            ("pacman", Manager::Pacman),
            ("zypper", Manager::Zypper),
            ("apk", Manager::Apk),
        ] {
            if detector.is_command(cmd) {
                return m;
            }
        }
        if detector.is_command("brew") { Manager::Brew } else { Manager::Apt }
    }

    fn install(self, pkg: &str) -> String {
        match self {
            Manager::Winget => format!("winget install -e --id {pkg}"),
            Manager::Brew => format!("brew install {pkg}"),
            Manager::Apt => format!("sudo apt-get install -y {pkg}"),
            Manager::Dnf => format!("sudo dnf install -y {pkg}"),
            Manager::Pacman => format!("sudo pacman -S --needed {pkg}"),
            Manager::Zypper => format!("sudo zypper install -y {pkg}"),
            Manager::Apk => format!("sudo apk add {pkg}"),
        }
    }

    fn search(self, name: &str) -> String {
        match self {
            Manager::Winget => format!("winget search {name}"),
            Manager::Brew => format!("brew search {name}"),
            Manager::Apt => format!("apt-cache search {name}"),
            Manager::Dnf => format!("dnf search {name}"),
            Manager::Pacman => format!("pacman -Ss {name}"),
            Manager::Zypper => format!("zypper search {name}"),
            Manager::Apk => format!("apk search {name}"),
        }
    }
}

/// Commands -> package per manager: (commands, winget, brew, apt, dnf, pacman). "" means not packaged there.
/// zypper borrows the dnf name and apk the apt name; close enough for a suggestion the user can edit.
const PACKAGES: &[(&[&str], &str, &str, &str, &str, &str)] = &[
    (&["aws"], "Amazon.AWSCLI", "awscli", "awscli", "awscli2", "aws-cli-v2"),
    (&["az"], "Microsoft.AzureCLI", "azure-cli", "azure-cli", "azure-cli", "azure-cli"),
    (&["gcloud", "gsutil"], "Google.CloudSDK", "--cask google-cloud-sdk", "google-cloud-cli", "google-cloud-cli", ""),
    (&["node", "npm", "npx"], "OpenJS.NodeJS.LTS", "node", "nodejs npm", "nodejs npm", "nodejs npm"),
    (&["python", "python3", "pip", "pip3"], "Python.Python.3.13", "python", "python3 python3-pip", "python3 python3-pip", "python python-pip"),
    (&["docker"], "Docker.DockerDesktop", "--cask docker", "docker.io", "moby-engine", "docker"),
    (&["kubectl"], "Kubernetes.kubectl", "kubectl", "kubectl", "kubernetes-client", "kubectl"),
    (&["helm"], "Helm.Helm", "helm", "helm", "helm", "helm"),
    (&["terraform"], "Hashicorp.Terraform", "hashicorp/tap/terraform", "terraform", "terraform", "terraform"),
    (&["gh"], "GitHub.cli", "gh", "gh", "gh", "github-cli"),
    (&["git"], "Git.Git", "git", "git", "git", "git"),
    (&["go"], "GoLang.Go", "go", "golang-go", "golang", "go"),
    (&["cargo", "rustc", "rustup"], "Rustlang.Rustup", "rustup", "rustup", "rustup", "rustup"),
    (&["java", "javac"], "EclipseAdoptium.Temurin.21.JDK", "openjdk", "default-jdk", "java-latest-openjdk", "jdk-openjdk"),
    (&["dotnet"], "Microsoft.DotNet.SDK.8", "--cask dotnet-sdk", "dotnet-sdk-8.0", "dotnet-sdk-8.0", "dotnet-sdk"),
    (&["deno"], "DenoLand.Deno", "deno", "", "", "deno"),
    (&["bun"], "Oven-sh.Bun", "oven-sh/bun/bun", "", "", ""),
    (&["pnpm"], "pnpm.pnpm", "pnpm", "", "", "pnpm"),
    (&["yarn"], "Yarn.Yarn", "yarn", "yarnpkg", "yarnpkg", "yarn"),
    (&["jq"], "jqlang.jq", "jq", "jq", "jq", "jq"),
    (&["yq"], "MikeFarah.yq", "yq", "yq", "yq", "go-yq"),
    (&["rg"], "BurntSushi.ripgrep.MSVC", "ripgrep", "ripgrep", "ripgrep", "ripgrep"),
    (&["fd"], "sharkdp.fd", "fd", "fd-find", "fd-find", "fd"),
    (&["fzf"], "junegunn.fzf", "fzf", "fzf", "fzf", "fzf"),
    (&["bat"], "sharkdp.bat", "bat", "bat", "bat", "bat"),
    (&["ffmpeg", "ffprobe"], "Gyan.FFmpeg", "ffmpeg", "ffmpeg", "ffmpeg-free", "ffmpeg"),
    (&["nvim"], "Neovim.Neovim", "neovim", "neovim", "neovim", "neovim"),
    (&["vim"], "vim.vim", "vim", "vim", "vim-enhanced", "vim"),
    (&["code"], "Microsoft.VisualStudioCode", "--cask visual-studio-code", "code", "code", "code"),
    (&["pwsh"], "Microsoft.PowerShell", "--cask powershell", "powershell", "powershell", ""),
    (&["make"], "ezwinports.make", "make", "make", "make", "make"),
    (&["cmake"], "Kitware.CMake", "cmake", "cmake", "cmake", "cmake"),
    (&["wget"], "JernejSimoncic.Wget", "wget", "wget", "wget", "wget"),
    (&["7z"], "7zip.7zip", "p7zip", "p7zip-full", "p7zip", "p7zip"),
    (&["htop"], "", "htop", "htop", "htop", "htop"),
    (&["tree"], "GnuWin32.Tree", "tree", "tree", "tree", "tree"),
    (&["psql"], "PostgreSQL.PostgreSQL.17", "libpq", "postgresql-client", "postgresql", "postgresql"),
    (&["mysql"], "Oracle.MySQL", "mysql-client", "mysql-client", "mysql", "mariadb-clients"),
    (&["redis-cli"], "", "redis", "redis-tools", "redis", "redis"),
    (&["sqlite3"], "SQLite.SQLite", "sqlite", "sqlite3", "sqlite", "sqlite"),
    (&["tmux"], "", "tmux", "tmux", "tmux", "tmux"),
    (&["ollama"], "Ollama.Ollama", "ollama", "", "", "ollama"),
];

pub enum Hint {
    /// A ready-to-run install command.
    Install(String),
    /// Nothing known for this name; a search the user could run instead.
    Search(String),
}

pub fn hint(name: &str, manager: Manager) -> Hint {
    let row = PACKAGES.iter().find(|(cmds, ..)| cmds.contains(&name));
    let pkg = row.map(|&(_, winget, brew, apt, dnf, pacman)| match manager {
        Manager::Winget => winget,
        Manager::Brew => brew,
        Manager::Apt | Manager::Apk => apt,
        Manager::Dnf | Manager::Zypper => dnf,
        Manager::Pacman => pacman,
    });
    match pkg {
        Some(p) if !p.is_empty() => Hint::Install(manager.install(p)),
        _ => Hint::Search(manager.search(name)),
    }
}

/// The program a command line would start: skips `VAR=value` prefixes and `sudo`.
pub fn program(line: &str) -> Option<&str> {
    line.split_whitespace()
        .find(|w| !(w.contains('=') && !w.starts_with('=')) && *w != "sudo")
        .filter(|w| !w.contains(['/', '\\', '$', '`', '(', '\'', '"']))
}

/// Is this line an install through a package manager (so PATH may have just changed)?
pub fn is_install(line: &str) -> bool {
    ["winget install", "scoop install", "choco install"].iter().any(|p| line.contains(p))
}

/// A winget install updates PATH in the registry, not in this process: pick the new value up so
/// the freshly installed command works without restarting aiterm.
#[cfg(windows)]
pub fn refresh_path() {
    let out = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "[Environment]::GetEnvironmentVariable('Path','Machine') + ';' + [Environment]::GetEnvironmentVariable('Path','User')",
        ])
        .output();
    if let Ok(o) = out {
        let path = String::from_utf8_lossy(&o.stdout).trim().to_string();
        if o.status.success() && !path.is_empty() {
            std::env::set_var("PATH", path);
        }
    }
}

#[cfg(not(windows))]
pub fn refresh_path() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hints() {
        assert!(matches!(hint("aws", Manager::Winget), Hint::Install(c) if c == "winget install -e --id Amazon.AWSCLI"));
        assert!(matches!(hint("aws", Manager::Brew), Hint::Install(c) if c == "brew install awscli"));
        assert!(matches!(hint("fd", Manager::Apt), Hint::Install(c) if c == "sudo apt-get install -y fd-find"));
        assert!(matches!(hint("htop", Manager::Winget), Hint::Search(_)));
        assert!(matches!(hint("nosuchtool", Manager::Pacman), Hint::Search(c) if c == "pacman -Ss nosuchtool"));
    }

    #[test]
    fn programs() {
        assert_eq!(program("aws --version"), Some("aws"));
        assert_eq!(program("AWS_PROFILE=x sudo aws s3 ls"), Some("aws"));
        assert_eq!(program("./run.sh"), None);
    }
}
