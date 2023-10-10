variable "store" {
  description = "npcnix_s3_store module to use"
}

variable "name" {
  description = "Name of the file to put in the store (e.g. 'dev.npcnix')"
}

locals {
  filename       = "${var.name}.npcnix"
}

output "filename" {
  value = local.filename
}

output "store" {
  value = var.store
}

output "bucket" {
  value = var.store.bucket
}

output "key" {
  value = "${var.store.prefix}/${local.filename}"
}

output "url" {
  value = "s3://${var.store.bucket.id}/${var.store.prefix}/${local.filename}"
}

output "region" {
  value = var.store.region
}
