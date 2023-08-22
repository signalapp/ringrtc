#!/usr/bin/env python3

#
# Copyright 2023 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

"""
This script builds webrtc artifacts for a specified target on preconfigured AWS EC2 instances
"""

try:
    import argparse
    import boto3
    import botocore
    import fabric
    import paramiko
    import time
except ImportError as e:
    raise ImportError(str(e) + '- required module not found')

TARGET_PLATFORMS = ['android', 'ios', 'linux', 'mac', 'windows']

PLATFORM_EC2_INSTANCE_TAG = {
    'android': 'webrtc-build-linux',
    'ios': 'webrtc-build-macos',
    'linux': 'webrtc-build-linux',
    'mac': 'webrtc-build-macos',
    'windows': 'webrtc-build-windows'
}

EC2_INSTANCE_TAG_USER = {
    'webrtc-build-linux': 'ubuntu',
    'webrtc-build-macos': 'ec2-user'
}

EC2_DEDICATED_MAC_HOST_TAG = 'webrtc-mac-host'


def parse_args():
    parser = argparse.ArgumentParser(
        description='Build webrtc on preconfigured AWS EC2 instances')
    parser.add_argument('--aws-profile',
                        required=True,
                        help='AWS profile name. Look for [profile <NAME>] in ~/.aws/config')
    parser.add_argument('--aws-region',
                        required=True,
                        help='AWS region. Example: us-east-2')
    parser.add_argument('--aws-identity-file',
                        required=True,
                        help='AWS EC2 keypair pem file path.')
    parser.add_argument('--keep-running',
                        action='store_true',
                        help='Keep ec2 instance running. Default: false')
    parser.add_argument('--target',
                        required=True,
                        help='build target: ' + ', '.join(TARGET_PLATFORMS))
    parser.add_argument('--webrtc',
                        required=True,
                        help='WebRTC version tag. Example: 5615d')
    return parser.parse_args()


def get_ec2_instance(ec2_tag: str):
    ec2 = boto3.resource('ec2')
    custom_filter = [{
        'Name': 'tag:Name', 'Values': [ec2_tag]}]

    instances = ec2.instances.filter(Filters=custom_filter)

    if len(list(instances.all())) == 0:
        raise RuntimeError(ec2_tag + " not found.")
    elif len(list(instances.all())) > 1:
        raise RuntimeError(ec2_tag + ": too many instances with this tag found.")
    return list(instances.all())[0]


def place_dedicated_host(instance, host_id: str):
    boto3.client('ec2').modify_instance_placement(
        Affinity='host',
        HostId=host_id,
        InstanceId=instance.id,
        Tenancy='host',
    )


def start_new_host(instance):
    availability_zone = instance.network_interfaces[0].subnet.availability_zone
    response = boto3.client('ec2').allocate_hosts(
        AvailabilityZone=availability_zone,
        InstanceType='mac2.metal',
        Quantity=1,
        TagSpecifications=[
            {
                'ResourceType': 'dedicated-host',
                'Tags': [
                    {
                        'Key': 'Name',
                        'Value': EC2_DEDICATED_MAC_HOST_TAG
                    },
                ]
            },
        ],
        HostMaintenance='off',
    )
    return response['HostIds'][0]


def setup_dedicated_host(instance):
    try:
        # Check whether a dedicated host exists
        first = True
        while True:
            response = boto3.client('ec2').describe_hosts(Filters=[
                {
                    'Name': 'tag:Name',
                    'Values': [
                        EC2_DEDICATED_MAC_HOST_TAG,
                    ]
                },
            ])
            if len(response['Hosts']) == 0:
                host_id = start_new_host(instance)
                place_dedicated_host(instance, host_id)
            elif len(response['Hosts'][0]['Instances']) == 0 or \
                    response['Hosts'][0]['Instances'][0]['InstanceId'] != instance.id:
                host = response['Hosts'][0]
                if host['State'] == 'pending':
                    if first:
                        first = False
                        print(f'Waiting for host {host["HostId"]} to become available...')
                    time.sleep(10)
                    continue
                else:
                    place_dedicated_host(instance, host['HostId'])
            break
    except botocore.exceptions.ClientError as error:
        if error.response['Error']['Code'] == 'InvalidHostID.NotFound':
            host_id = start_new_host(instance)
            place_dedicated_host(instance, host_id)
        else:
            raise error


def start_ec2_instance(target: str, ec2_tag: str):
    instance = get_ec2_instance(ec2_tag)

    # Set up a dedicated host for mac builds
    if target == 'mac' or target == 'ios':
        # Allocate dedicated host
        setup_dedicated_host(instance)

    # Wait for instance to be stopped or running
    if instance.state['Name'] != 'stopped' and instance.state['Name'] != 'running':
        print(f'{instance} state: {instance.state["Name"]}')
        print(f'{instance} stopping...')
        instance.stop()
        waiter = boto3.client('ec2').get_waiter('instance_stopped')
        waiter.wait(
            InstanceIds=[instance.id]
        )

    instance.start()
    print(f'{instance} initializing...')
    waiter = boto3.client('ec2').get_waiter('instance_status_ok')
    waiter.wait(
        InstanceIds=[instance.id],
        Filters=[
            {
                "Name": "instance-status.reachability",
                "Values": [
                    "passed"
                ]
            }
        ]
    )
    print(f'{instance} ready')
    return instance


def stop_ec2_instance(instance):
    instance.stop()
    print(f'{instance} stopped')


def build_webrtc(target, hostname, user, identity_filepath, webrtc):
    with fabric.Connection(hostname,
                           user,
                           connect_kwargs={"pkey": paramiko.RSAKey.from_private_key_file(identity_filepath)}) as conn:
        # Clone ringrtc
        conn.run('rm -rf ringrtc')
        conn.run('git clone --depth 1 https://github.com/signalapp/ringrtc.git')

        # Set webrtc version
        if target == 'android' or target == 'linux':
            conn.run(f'sed -i "/webrtc.version=/ s/=.*/={webrtc}/" ringrtc/config/version.properties')
        else:
            conn.run('sed -i \'\' "/webrtc.version=/ s/=.*/={}/" ringrtc/config/version.properties'.format(webrtc))

        # Build target and download resulting artifacts
        conn.run(f'cd ringrtc; ./bin/build-webrtc.py --target {target} --release')
        if target == 'android' or target == 'ios':
            conn.get(f'ringrtc/out/webrtc-{webrtc}-{target}-release.tar.bz2')
        elif target == 'linux' or target == 'mac':
            conn.get(f'ringrtc/out/webrtc-{webrtc}-{target}-x64-release.tar.bz2')
            conn.get(f'ringrtc/out_arm/webrtc-{webrtc}-{target}-arm64-release.tar.bz2')


def main() -> None:
    args = parse_args()

    # Setup AWS profile
    boto3.setup_default_session(profile_name=args.aws_profile, region_name=args.aws_region)

    if args.target == 'windows':
        raise Exception(f'{args.target} not implemented')

    # Start instance
    ec2_tag = PLATFORM_EC2_INSTANCE_TAG[args.target]
    instance = start_ec2_instance(args.target, ec2_tag)

    # Build target on instance
    print(f'Building webrtc on instance {instance.id}')
    build_webrtc(
        args.target, instance.public_dns_name, EC2_INSTANCE_TAG_USER[ec2_tag], args.aws_identity_file, args.webrtc)

    # Stop instance
    if args.keep_running:
        print(f'EC2 instance {instance.id} will keep running')
    else:
        stop_ec2_instance(instance)


if __name__ == '__main__':
    main()
