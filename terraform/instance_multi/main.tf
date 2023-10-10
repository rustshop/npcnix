terraform {
  required_providers {
    aws = {
      source = "hashicorp/aws"
    }
  }
}

variable "remote" {}
variable "configuration" {
  default = null
}
variable "install" {
  default = null
}

variable "root_access" {
  default = false
  type    = bool
}
variable "root_ssh_keys" {
  default = []
  type    = list(string)
}

variable "pre_install_script" {
  default = ""
}

variable "post_install_script" {
  default = ""
}

variable "iam_policies" {
  type    = map(any)
  default = {}
}

variable "hosts" {
  # {
  #   "name" = {
  #      subnet =
  #  
  #   }
  # }
}


variable "hostname_base" {}
variable "dns_zone" {}
variable "vpc" {}
variable "eip" {
  default = false
}

variable "ami" {}
variable "instance_type" { default = "t3.micro" }
variable "root_volume_size" {
  default = 8
}

variable "public_tcp_ports" {
  default = []
}
variable "internal_tcp_ports" {
  default = [22]
}

output "instance" {
  value = aws_instance.instance
}


locals {
  append_root_ssh_keys_cmd = join("\n",
    [for key in var.root_ssh_keys : "echo \"${key}\" >> /root/.ssh/authorized_keys"]
  )

  clear_root_ssh_keys_cmd = <<EOF
mkdir -p /root/.ssh
echo "" > /root/.ssh/authorized_keys
chown root: /root/.ssh/authorized_keys
chmod 0600 /root/.ssh/authorized_keys
EOF

  write_root_ssh_keys_cmd = <<EOF
${local.clear_root_ssh_keys_cmd}
${local.append_root_ssh_keys_cmd}
EOF
}

resource "aws_eip" "eip" {
  for_each = var.eip ? var.hosts : {}
  vpc      = true

  tags = {
    Name = "${each.key}.${var.hostname_base}"
  }
}

resource "aws_eip_association" "eip_association" {
  for_each      = var.eip ? var.hosts : {}
  instance_id   = aws_instance.instance[each.key].id
  allocation_id = aws_eip.eip[each.key].id
}


resource "aws_iam_role" "iam_role" {
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Action = "sts:AssumeRole"
        Effect = "Allow"
        Sid    = ""
        Principal = {
          Service = "ec2.amazonaws.com"
        }
      },
    ]
  })
}

resource "aws_iam_instance_profile" "iam_instance_profile" {
  role = aws_iam_role.iam_role.name
}

resource "aws_iam_role_policy_attachment" "npcnix_store_access" {
  role       = aws_iam_role.iam_role.name
  policy_arn = var.remote.store.iam_policy.arn
}

resource "aws_iam_role_policy_attachment" "policies" {
  for_each   = var.iam_policies
  role       = aws_iam_role.iam_role.name
  policy_arn = each.value.arn
}

resource "aws_instance" "instance" {
  for_each             = var.hosts
  ami                  = var.ami
  instance_type        = var.instance_type
  iam_instance_profile = aws_iam_instance_profile.iam_instance_profile.id

  root_block_device {
    volume_size = var.root_volume_size
    volume_type = "gp3"
  }

  subnet_id = each.value.subnet.id

  vpc_security_group_ids = [aws_security_group.instance.id]

  user_data = <<EOF
#!/usr/bin/env bash
${local.clear_root_ssh_keys_cmd}
%{if var.root_access}
${local.append_root_ssh_keys_cmd}
%{endif}
%{if var.install != null}
%{if var.configuration != null}
remote_url='${var.remote.url}'
remote_region='${var.remote.region}'
configuration='${var.configuration}'
%{else}
remote_url='${var.remote.url}'
remote_region='${var.remote.region}'
configuration='${each.key}-${var.hostname_base}'
%{endif}

${var.pre_install_script}
${var.install.install_script}
${var.post_install_script}
  
%{endif}
EOF

  tags = {
    Name = "${each.key}.${var.hostname_base}"
  }
}

resource "aws_security_group" "instance" {
  vpc_id = var.vpc.id

  egress = [
    {
      description      = "outgoing"
      cidr_blocks      = ["0.0.0.0/0", ]
      from_port        = 0
      to_port          = 0
      ipv6_cidr_blocks = ["::/0"]
      prefix_list_ids  = []
      protocol         = "-1"
      security_groups  = []
      self             = false
    }
  ]
  ingress = concat([for port in var.public_tcp_ports :
    {
      description      = "${port}"
      cidr_blocks      = ["0.0.0.0/0", ]
      from_port        = port
      to_port          = port
      ipv6_cidr_blocks = ["::/0"]
      prefix_list_ids  = []
      protocol         = "tcp"
      security_groups  = []
      self             = false
    }], [for port in var.internal_tcp_ports :
    {
      description      = "${port}"
      cidr_blocks      = [var.vpc.cidr_block, ]
      from_port        = port
      to_port          = port
      ipv6_cidr_blocks = [var.vpc.ipv6_cidr_block]
      prefix_list_ids  = []
      protocol         = "tcp"
      security_groups  = []
      self             = false
  }])
}

resource "aws_route53_record" "instance-aaaa" {
  for_each = var.hosts
  zone_id  = var.dns_zone.zone_id
  name     = "private.${each.key}.${var.hostname_base}.${var.dns_zone.name}"
  type     = "AAAA"
  ttl      = "300"
  records  = aws_instance.instance[each.key].ipv6_addresses
}

resource "aws_route53_record" "instance-a" {
  for_each = var.hosts
  zone_id  = var.dns_zone.zone_id
  name     = "private.${each.key}.${var.hostname_base}.${var.dns_zone.name}"
  type     = "A"
  ttl      = "300"
  records  = [aws_instance.instance[each.key].private_ip]
}
