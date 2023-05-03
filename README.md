# NPCNIX

> Control your NixOS instances system configuration from a centrally managed location.

## Overview

If you are already using NixOS flakes to configure your NixOS
systems, why bother using ssh to change their configurations,
if you could just ... let them configure themselves automatically,
on their own.

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

All the terraform modules used here are in the `./terraform` directory. You should
probably pin them, or even just use as a reference.

So first, we need a bucket to store the config:

```terraform
resource "aws_s3_bucket" "config" {
  bucket = "some-config"
}

resource "aws_s3_bucket_public_access_block" "config" {
  bucket = aws_s3_bucket.config.id

  block_public_acls   = true
  block_public_policy = true
  ignore_public_acls  = true
}
```

And then let's carve out a part of it for *remotes* (compressed Nix flakes):

```terraform
module "npcnix_s3_store" {
  source = "github.com/rustshop/npcnix//terraform/store_s3?ref=a1dd4621a56724fe36ca8940eb7172dd0f4be986"

  bucket = aws_s3_bucket.config
  prefix = "npcnix/remotes"
}
```

We're going to need a boot script:

```terraform
module "npcnix_install" {
  source = "github.com/rustshop/npcnix//terraform/install?ref=a1dd4621a56724fe36ca8940eb7172dd0f4be986"
}
```

Finally a remote along with the command that will pack and upload to it:

```terraform
module "remote_dev" {
  source   = "github.com/rustshop/npcnix//terraform/remote_s3?ref=a1dd4621a56724fe36ca8940eb7172dd0f4be986"

  name  = "dev"
  store = var.npcnix_s3_store
}

module "remote_dev_upload" {
  source   = "github.com/rustshop/npcnix//terraform/remote_s3_upload?ref=a1dd4621a56724fe36ca8940eb7172dd0f4be986"

  remote    = module.remote_dev
  flake_dir = "../../configurations"
  include   = []
}
```

And a EC2 instance that will bootstrap itself using the install script,
have some alternative root ssh access (for debugging any issues) and
then configure itself to use `"host"` NixOS configuration from flake
in `../../configurations`.

```terraform
module "host" {
  source = "github.com/rustshop/npcnix//terraform/instance?ref=a1dd4621a56724fe36ca8940eb7172dd0f4be986"

  providers = {
    aws = aws
  }

  remote        = module.remote_dev
  install       = module.npcnix_install
  root_access   = true
  root_ssh_keys = local.fallback_ssh_keys

  hostname      = "host"
  subnet        = module.vpc-us-east-1.subnets["public-a"]
  dns_zone      = aws_route53_zone.dev
  ami           = local.ami.nixos_22_11.us-east-1
  instance_type = "t3.nano"

  pre_install_script = local.user_data_network_init

  public_tcp_ports = [22]
}
```

And that's basically it for Terraform configuration required.

On `terraform apply`, local `npcnix pack` will pack the Nix flake from `../../configurations`, and upload it to a remote. On start the system daemon will execute script prepared by `npcnix_install` that will configure `npcnix` on the machine, download the packed flake, and switch the configuration. As long as that configuration has a npcnix NixOS module enabled, a system daemon will keep monitoring the remote and switching to the desired configuration. 

With just one command, you can start one or more machines that will automatically provision themselves with the desired configuration.

## FAQ

### What about destination machines having to build each configuration?

Use a build cache and/or remote builder machine. Both the install script module and the npcnix itself can use it. You can populate it from your local machine or CI of some kind.
