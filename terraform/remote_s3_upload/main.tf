variable "remote" {
}

variable "local_dst_dir" {
  default = null
}

variable "flake_dir" {
  description = "Directory containing flake.nix file to upload (e.g. '../../nixos')"
}

variable "include" {
  description = "Subdirectories to include"
  type        = list(string)
}

locals {
  local_dst_dir = var.local_dst_dir != null ? var.local_dst_dir : path.root
}

# generate nixos config file
data "external" "npcnix-pack" {
  program = concat(
    ["${path.module}/bin/md5-out-wrap", "${local.local_dst_dir}/${var.remote.filename}"],
    ["npcnix", "pack", "--src", "${var.flake_dir}", "--dst", "${local.local_dst_dir}/${var.remote.filename}"],
    flatten([for dir in var.include : ["--include", dir]])
  )
}


# upload the config to npcnix location
resource "aws_s3_object" "remote" {
  bucket = var.remote.store.bucket.id
  key    = "${var.remote.store.prefix}/${var.remote.filename}"

  source = data.external.npcnix-pack.result.path
  etag   = data.external.npcnix-pack.result.md5sum
}
