# AI permissions

How Tripppin decides what the AI may do as administrator. The UI is in Settings → AI →
Permissions; the code is in `crates/trip/src/permissions.rs`, `os/files/usr/libexec/tripppin-ai-sudo`
and the sudo wrapper in `crates/trip/src/jobs.rs`.

## Why not a separate user for the AI

A dedicated Linux user with extra rights sounds tidy, but the AI works on the person's files,
apps and desktop session. A second user couldn't see them without broad group or ACL changes,
and anything it created would belong to someone else. So the AI runs as the person, and
administrator rights go through sudo, which already does what we need: per-command rules,
password prompts and logging.

## The model

A **level**, plus **areas** at the middle level:

| Level | What happens |
|---|---|
| Ask me every time (default) | Every `sudo` shows the pop-up with the command and asks for the password. |
| Allow what I pick | The areas switched on run without asking; everything else asks. |
| Full administrator | Everything runs without asking, except risky commands, which still ask. |

| Area | Commands | Default when picking |
|---|---|---|
| Apps and packages | dnf, dnf5, flatpak, rpm-ostree | on |
| System services | systemctl, journalctl | on |
| Updates | bootc | on |
| Network and firewall | nmcli, firewall-cmd | off |
| Hardware and drivers | modprobe, fwupdmgr, dmidecode | off |
| Disks and storage | mount, umount, udisksctl mount/unmount/info/status, lsblk, blkid | off |
| System settings files | `/usr/libexec/tripppin-etc-write` | off |
| Users and groups | useradd, usermod, userdel, group*, gpasswd, passwd, chpasswd | off |
| Printers | lpadmin, lpinfo, cups{enable,disable,accept,reject} | on |

**Risky at Full administrator** (they still ask for the password): rm, shred, dd, wipefs, mkfs*,
fdisk/sfdisk/gdisk/sgdisk/parted, blkdiscard, cryptsetup, `udisksctl format`, visudo, every user
and group command, stopping/disabling/masking firewalld, `firewall-cmd --panic-on` and making
`trusted` the default zone.

**System settings files** go through `tripppin-etc-write`: it writes standard input to one file
under `/etc`, refuses passwords, shadow, sudoers, PAM, polkit, SELinux, fstab, SSH keys and our
own AI policy, keeps the old file in `/var/lib/tripppin/etc-backups`, and keeps owner, mode and
SELinux label.

## Where it lives

- `/etc/tripppin/ai-permissions` (root, 0644): the policy, one TOML section per person:

  ```toml
  [user."akshay"]
  level = "pick"
  areas = ["apps", "services", "printers"]
  ```

- `/etc/sudoers.d/tripppin-ai-<user>` (root, 0440): the rule made from it. `ask` removes it;
  `pick` is one `NOPASSWD:` line with the areas' commands; `full` is `NOPASSWD: ALL` followed by
  a `PASSWD:` line for the risky commands (sudo's last match wins).

Settings runs `pkexec /usr/libexec/tripppin-ai-sudo --level <ask|pick|full> [area…]`, so an
administrator types their password once per change. The helper only accepts people in `wheel`,
checks the new rule with `visudo -cf`, installs it, checks the whole configuration with
`visudo -c` (and takes the rule out again if that fails), rewrites only that person's section of
the policy, and logs the change to the journal.

**Compatibility.** The old form (`tripppin-ai-sudo install services updates` / `off`) still
works and maps to `pick` with apps/services/updates. When no policy file exists yet, Settings
and the agent read the old `/etc/tripppin/ai-sudo` the same way. The old single rule
`/etc/sudoers.d/tripppin-ai` is removed the first time its owner saves new permissions.

## How the agent uses sudo

Every command the agent runs gets a `sudo` shell function (`permissions::sudo_wrapper`):

- With no terminal (the orb, background jobs) it runs `sudo -k -A`: `-A` asks through the
  pop-up (`tripppin-askpass` → `tripppin-ui askpass`), and `-k` means a password the person typed
  earlier in a terminal is never reused, so "ask every time" really is every time.
- It appends one line per call to `~/.local/state/tripppin/admin.log`: time, exit status, what
  the pop-up recorded (`asked`, `cancelled`, nothing when sudo didn't ask, `terminal`), and the
  command. The pop-up writes its answer to a per-call file named by `TRIPPPIN_SUDO_MARK`; it
  never sees or writes anything else.
- The pop-up warns in red when the command looks risky (`permissions::is_risky`).

Settings → AI → Activity shows the newest entries: "Allowed by your settings", "You allowed it",
"You said no", "Didn't succeed", or "In the terminal".

The harness can tell the model its permissions with `trip::permissions::summary()` (a few
sentences: what runs without asking, that other commands ask, how to edit `/etc`, and not to
work around a refusal).

## Limits, honestly

- This is about consent and accidents, not a sandbox against a hostile model. Several allowed
  tools can be turned into a root shell (systemctl, dnf, mount, `NOPASSWD: ALL` with
  exceptions), and the risky list matches commands by path, so `sudo sh -c 'rm …'` at Full
  administrator isn't caught. The pop-up and the activity log are what keep it visible.
- sudoers can't tell the AI from the person, so what's allowed also runs without a password for
  the person in a terminal. Settings says so.
- "A confirmation tap" for risky commands at Full administrator is a password prompt: sudo can't
  ask for a tap without a custom PAM module. That could come later.
- The activity log is in the person's home folder, so it's a convenience, not an audit trail;
  sudo's own entries in the journal (`journalctl _COMM=sudo`) are the authoritative record.
