{ pkgs, ... }:

{
  # https://devenv.sh/basics/
  env.GREET = "devenv";
  env.MOZ_REMOTE_SETTINGS_DEVTOOLS=1;

  # https://devenv.sh/packages/
  packages = [ pkgs.git pkgs.geckodriver pkgs.firefox ];

  # https://devenv.sh/scripts/
  scripts.hello.exec = "echo hello from $GREET";

  # https://devenv.sh/processes/
  processes.geckodriver.exec = "geckodriver";
}
