//go:build !windows

package main

import "os/exec"

func configureCoreCommand(_ *exec.Cmd) {}
