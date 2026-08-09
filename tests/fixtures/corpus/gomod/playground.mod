module example.com/deploy-agent

go 1.22

require (
	github.com/pkg/errors v0.9.1
	golang.org/x/sync v0.7.0
	github.com/spf13/cobra v1.8.0
	codeberg.org/solo/widget v1.2.3
)

replace example.com/legacy => ../legacy

exclude golang.org/x/net v0.17.0
