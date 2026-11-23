const { identity } = require("./identity");

function double(value) {
  return identity(value) * 2;
}

module.exports = { double };
