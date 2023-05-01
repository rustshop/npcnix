variable "npcnix_install_url" {
  type = string
  default = "github:rustshop/npcnix?rev=421c1f3c38bed2ca4af54a659d6e64e71e5e146c#install"
}

variable "extra_substituters" {
  type = list(string)
  default = ["https://rustshop.cachix.org"]
}

variable "extra_trusted_public_keys" {
  type = list(string)
  default = ["rustshop.cachix.org-1:VD3xhDANGzOZTKuGPHcW7KOTZS0DPoPQSxXB00Yt0ZQ="]
}

output "install_script" {
  value = <<EOF
nix \
  --extra-experimental-features nix-command \
  --extra-experimental-features flakes \
%{for s in var.extra_substituters ~}
  --option extra-substituters "${s}" \
%{endfor ~}
%{for s in var.extra_trusted_public_keys ~}
  --option extra-trusted-public-keys "${s}" \
%{endfor ~}
  run \
   -L \
  ${var.npcnix_install_url} -- \
%{for s in var.extra_substituters ~}
  --extra-substituters "${s}" \
%{endfor ~}
%{for s in var.extra_trusted_public_keys ~}
  --extra-trusted-public-keys "${s}" \
%{endfor ~}
  --remote "$remote_url" \
  --configuration "$configuration"
EOF
}

