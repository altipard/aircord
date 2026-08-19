package playbook

import (
	"regexp"
	"strings"
)

// riskyCmds matches AT commands that reboot, reset or factory-wipe a device.
// Running one on a sensor that is already deployed in the field costs a second
// site visit, so the UI asks before replaying a playbook containing any of them.
//
// The list is deliberately conservative: it flags the well-known destructive
// commands across the Dragino / Quectel families rather than trying to guess at
// vendor-specific syntax. A command not listed here is not thereby safe — it is
// only not known to be destructive.
var riskyCmds = regexp.MustCompile(`(?i)^AT(Z|\+(FDR|FACNEW|FACTORY|RESET|REBOOT|CFACTORY|QRST))\b`)

// Risky reports whether cmd is a known reboot / reset / factory-wipe command.
func Risky(cmd string) bool {
	return riskyCmds.MatchString(strings.TrimSpace(cmd))
}

// RiskySteps returns the commands in steps that Risky flags, in order and
// without duplicates, so a confirmation prompt can name exactly what is about to
// happen instead of warning in the abstract.
func RiskySteps(steps []Step) []string {
	var out []string
	seen := make(map[string]bool, len(steps))
	for _, st := range steps {
		cmd := strings.TrimSpace(st.Cmd)
		if !Risky(cmd) || seen[cmd] {
			continue
		}
		seen[cmd] = true
		out = append(out, cmd)
	}
	return out
}
