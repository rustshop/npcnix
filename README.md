# NPCNIX

> Provision your NixOS systems by broadcasting the official
> narrative through the approved official communication channels.

## Overview

If you are already using NixOS flakes to configure your NixOS
systems, why bother using ssh to change their configurations,
if you could just ... let them configure (and keep reconfiguring)
themselves automatically, on their own.

The plan is as follows:

First, prepare a location that can store and serve files
(an npcnix *store*) - e.g. an S3 bucket (or a prefix inside it).

Within it, publish compressed flakes under a certain
addresses (an npcnix *remote*s) - e.g. a keys in a S3 bucket.

Configure your NixOS hosts/images to run an initialization script
at the first boot that will download a flake from a given *remote*
and switch to a given NixOS *configuration* inside it, or use
a pre-built system image that includes `npcnix` support.

Each NixOS *configuration* should enable a `npcnix` system daemon,
that will periodically check (and reconfigure if needed) the system
following updates published in the *remote*.

Not exactly a Kubernetes cluster but with a somewhat similar
approach of agentes reacting to updates published in a central
location. Yet simpler, easy to set up, understand and customize,
and can go a long way to help manage a small to medium size herd
of NixOS-based machines.

It integrates well with existing infrastucture, CI systems and
tooling, especially in cloud environements.

In combination with internal remote builders and Nix caching
servers (or corresponding services like cachix) it can work very
effectively.

Since the npcnix-managed systems "pull" their configuration,
security posture of the whole system can be improved (in comparison
to a ssh-based approach), as active remote access is not even necessary,
and permission system can be centralized around the *store* write privileges.


## Setting up in AWS using Terraform

My use case involves AWS as a cloud, S3 as a cheap, yet abundant *store*,
with a built-in and integrated (in AWS) permission system, and Terraform
integration for convenience.

The guide will assume you're familiar with with the products and tools
used.

It shouldn't be difficult with a bit of cloud/system administraction
skills to implement `npcnix` in any other environment.

